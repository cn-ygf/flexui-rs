//! 用系统图形接口 Cairo(2D) + Pango(文字) 实现 `flexui_gfx::Canvas`。
//!
//! 渲染目标是内存中的 `cairo::ImageSurface`（ARGB32，物理像素）。窗口层每帧把它的
//! 像素 blit 到 X11 窗口。坐标一律「逻辑点、左上原点、y 向下」——构造时对 context
//! 施加 `scale`，之后所有绘制都用逻辑坐标；Cairo 原生支持 save/restore/clip。

use std::rc::Rc;

use cairo::{Context, Format, ImageSurface, LinearGradient};
use flexui_gfx::{
    Canvas, Color, Corners, Font, ImageFit, ImageSource, LayerHandle, Point, Rect, Size,
    TextBoundary, TextLayout,
};

/// Cairo 画布：持有绑定到目标 surface 的 context（引用计数，无需生命周期）。
pub struct CairoCanvas {
    cr: Context,
    /// 像素密度（HiDPI）。绘制用逻辑坐标，context 已按此缩放。
    scale: f32,
}

impl CairoCanvas {
    /// 用一块 ARGB32 ImageSurface 建画布。`scale`=物理像素/逻辑点。
    pub fn new(surface: &ImageSurface, scale: f32) -> Self {
        let cr = Context::new(surface).expect("cairo context");
        let s = scale.max(0.01) as f64;
        cr.scale(s, s);
        Self { cr, scale }
    }

    /// 建一块逻辑尺寸为 `size`、按 `scale` 放大的离屏 ARGB32 surface。
    fn new_surface(size: Size, scale: f32) -> Option<ImageSurface> {
        let pw = (size.width * scale).ceil().max(1.0) as i32;
        let ph = (size.height * scale).ceil().max(1.0) as i32;
        ImageSurface::create(Format::ARgb32, pw, ph).ok()
    }

    fn set_color(&self, c: Color) {
        self.cr
            .set_source_rgba(c.r as f64, c.g as f64, c.b as f64, c.a as f64);
    }

    /// 生成一个圆角矩形路径到当前 context（不描边/填充）。
    fn round_rect_path(&self, rect: Rect, radius: Corners) {
        let cr = &self.cr;
        let x = rect.left() as f64;
        let y = rect.top() as f64;
        let w = rect.size.width as f64;
        let h = rect.size.height as f64;
        let max_r = (w.min(h) / 2.0).max(0.0);
        let tl = (radius.tl as f64).clamp(0.0, max_r);
        let tr = (radius.tr as f64).clamp(0.0, max_r);
        let br = (radius.br as f64).clamp(0.0, max_r);
        let bl = (radius.bl as f64).clamp(0.0, max_r);
        use std::f64::consts::PI;
        cr.new_sub_path();
        cr.arc(x + w - tr, y + tr, tr, -PI / 2.0, 0.0);
        cr.arc(x + w - br, y + h - br, br, 0.0, PI / 2.0);
        cr.arc(x + bl, y + h - bl, bl, PI / 2.0, PI);
        cr.arc(x + tl, y + tl, tl, PI, 1.5 * PI);
        cr.close_path();
    }

    /// 用给定字体建一个 Pango 布局并设文本。
    fn pango_layout(&self, text: &str, font: &Font) -> pango::Layout {
        let layout = pangocairo::functions::create_layout(&self.cr);
        layout.set_font_description(Some(&font_description(font)));
        layout.set_text(text);
        layout
    }
}

/// flexui Font → Pango 字体描述。字号用绝对像素尺寸（逻辑点）。
fn font_description(font: &Font) -> pango::FontDescription {
    let mut fd = pango::FontDescription::new();
    if let Some(family) = &font.family {
        fd.set_family(family);
    } else {
        fd.set_family("sans-serif");
    }
    fd.set_absolute_size(font.size as f64 * pango::SCALE as f64);
    if font.bold {
        fd.set_weight(pango::Weight::Bold);
    }
    if font.italic {
        fd.set_style(pango::Style::Italic);
    }
    fd
}

impl Canvas for CairoCanvas {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.set_color(color);
        self.cr.rectangle(
            rect.left() as f64,
            rect.top() as f64,
            rect.size.width as f64,
            rect.size.height as f64,
        );
        let _ = self.cr.fill();
    }

    fn stroke_rect(&mut self, rect: Rect, color: Color, line_width: f32) {
        self.set_color(color);
        self.cr.set_line_width(line_width as f64);
        self.cr.rectangle(
            rect.left() as f64,
            rect.top() as f64,
            rect.size.width as f64,
            rect.size.height as f64,
        );
        let _ = self.cr.stroke();
    }

    fn fill_round_rect(&mut self, rect: Rect, radius: Corners, color: Color) {
        self.round_rect_path(rect, radius);
        self.set_color(color);
        let _ = self.cr.fill();
    }

    fn stroke_round_rect(&mut self, rect: Rect, radius: Corners, color: Color, line_width: f32) {
        self.round_rect_path(rect, radius);
        self.set_color(color);
        self.cr.set_line_width(line_width as f64);
        let _ = self.cr.stroke();
    }

    fn fill_gradient_rect(
        &mut self,
        rect: Rect,
        radius: Corners,
        from: Color,
        to: Color,
        vertical: bool,
    ) {
        let (x0, y0, x1, y1) = if vertical {
            (
                rect.left() as f64,
                rect.top() as f64,
                rect.left() as f64,
                rect.bottom() as f64,
            )
        } else {
            (
                rect.left() as f64,
                rect.top() as f64,
                rect.right() as f64,
                rect.top() as f64,
            )
        };
        let grad = LinearGradient::new(x0, y0, x1, y1);
        grad.add_color_stop_rgba(0.0, from.r as f64, from.g as f64, from.b as f64, from.a as f64);
        grad.add_color_stop_rgba(1.0, to.r as f64, to.g as f64, to.b as f64, to.a as f64);
        self.round_rect_path(rect, radius);
        let _ = self.cr.set_source(&grad);
        let _ = self.cr.fill();
    }

    fn draw_text(&mut self, text: &str, origin: Point, font: &Font, color: Color) {
        if text.is_empty() {
            return;
        }
        let layout = self.pango_layout(text, font);
        self.set_color(color);
        self.cr.move_to(origin.x as f64, origin.y as f64);
        pangocairo::functions::show_layout(&self.cr, &layout);
    }

    fn measure_text(&self, text: &str, font: &Font) -> Size {
        let layout = self.pango_layout(text, font);
        let (w, h) = layout.pixel_size();
        Size::new(w as f32, h as f32)
    }

    fn layout_text(&self, text: &str, font: &Font) -> TextLayout {
        let layout = self.pango_layout(text, font);
        let (w, h) = layout.pixel_size();
        let baseline = layout.baseline() as f32 / pango::SCALE as f32;
        let ascent = baseline;
        let descent = (h as f32 - ascent).max(0.0);

        // 每个字符左边界的 x（cursor 前置位置）。
        let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
        let mut char_index = 0usize;
        boundaries.push(TextBoundary {
            char_index: 0,
            x: 0.0,
        });
        for (byte_index, _) in text.char_indices() {
            if byte_index == 0 {
                continue;
            }
            let (strong, _weak) = layout.cursor_pos(byte_index as i32);
            char_index += 1;
            boundaries.push(TextBoundary {
                char_index,
                x: strong.x() as f32 / pango::SCALE as f32,
            });
        }
        // 末尾边界（文本长度处）。
        char_index += 1;
        boundaries.push(TextBoundary {
            char_index,
            x: w as f32,
        });

        TextLayout::new(
            text,
            font.clone(),
            Size::new(w as f32, h as f32),
            ascent,
            descent,
            boundaries,
        )
    }

    fn draw_text_layout(&mut self, layout: &TextLayout, origin: Point, color: Color) {
        self.draw_text(layout.text(), origin, layout.font(), color);
    }

    fn draw_image(
        &mut self,
        _source: &ImageSource,
        _rect: Rect,
        _tint: Option<Color>,
        _fit: ImageFit,
    ) {
        // 图片解码在后续阶段接入（image crate + SVG 光栅化）。
    }

    fn save(&mut self) {
        let _ = self.cr.save();
    }

    fn restore(&mut self) {
        let _ = self.cr.restore();
    }

    fn clip_rect(&mut self, rect: Rect) {
        self.cr.rectangle(
            rect.left() as f64,
            rect.top() as f64,
            rect.size.width as f64,
            rect.size.height as f64,
        );
        self.cr.clip();
    }

    fn clip_round_rect(&mut self, rect: Rect, radius: Corners) {
        self.round_rect_path(rect, radius);
        self.cr.clip();
    }

    fn scale(&self) -> f32 {
        self.scale
    }

    fn capture_layer(
        &mut self,
        size: Size,
        draw: &mut dyn FnMut(&mut dyn Canvas),
    ) -> Option<LayerHandle> {
        let surface = Self::new_surface(size, self.scale)?;
        {
            let mut off = CairoCanvas::new(&surface, self.scale);
            draw(&mut off);
        }
        surface.flush();
        Some(LayerHandle::new(
            size,
            self.scale,
            Rc::new(CairoLayer(surface)),
        ))
    }

    fn draw_layer(&mut self, layer: &LayerHandle, origin: Point) {
        let Some(CairoLayer(surface)) = layer.data::<CairoLayer>() else {
            return;
        };
        // surface 是按 scale 放大的物理像素；这里在逻辑坐标系（已 scale）里贴回，
        // 需先把 source 反缩放到逻辑尺寸。
        let _ = self.cr.save();
        self.cr.translate(origin.x as f64, origin.y as f64);
        let inv = 1.0 / self.scale.max(0.01) as f64;
        self.cr.scale(inv, inv);
        let _ = self.cr.set_source_surface(surface, 0.0, 0.0);
        let _ = self.cr.paint();
        let _ = self.cr.restore();
    }
}

/// 离屏图层后端载体：持有 cairo ImageSurface。
struct CairoLayer(ImageSurface);

#[cfg(test)]
mod tests {
    use super::*;

    /// 读某像素的 (R,G,B,A)。ARGB32 小端内存序为 B,G,R,A（预乘）。
    fn argb_at(surface: &mut ImageSurface, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let stride = surface.stride();
        let data = surface.data().unwrap();
        let idx = (y * stride + x * 4) as usize;
        (data[idx + 2], data[idx + 1], data[idx], data[idx + 3])
    }

    #[test]
    fn 填充矩形像素正确() {
        let surface = ImageSurface::create(Format::ARgb32, 20, 20).unwrap();
        {
            let mut cv = CairoCanvas::new(&surface, 1.0);
            cv.fill_rect(Rect::new(0.0, 0.0, 20.0, 20.0), Color::rgba(1.0, 0.0, 0.0, 1.0));
        }
        let mut surface = surface;
        let (r, g, b, a) = argb_at(&mut surface, 10, 10);
        assert!(r > 200 && g < 50 && b < 50 && a > 200, "{r},{g},{b},{a}");
    }

    #[test]
    fn 文字测量非空且边界单调() {
        let surface = ImageSurface::create(Format::ARgb32, 4, 4).unwrap();
        let cv = CairoCanvas::new(&surface, 1.0);
        let font = Font::system(16.0);
        let size = cv.measure_text("Hello", &font);
        assert!(size.width > 0.0 && size.height > 0.0, "{size:?}");
        let layout = cv.layout_text("Hello", &font);
        let bs = layout.boundaries();
        assert_eq!(bs.len(), 6, "5 字符应有 6 个边界");
        for pair in bs.windows(2) {
            assert!(pair[1].x >= pair[0].x - 0.01, "边界应单调不减");
        }
        assert!((bs[5].x - size.width).abs() < 1.0, "末边界≈宽度");
    }

    #[test]
    fn 离屏带_capture与blit() {
        // 主 surface 先铺蓝，再把一张红色离屏层贴到左上角。
        let surface = ImageSurface::create(Format::ARgb32, 30, 30).unwrap();
        {
            let mut cv = CairoCanvas::new(&surface, 1.0);
            cv.fill_rect(Rect::new(0.0, 0.0, 30.0, 30.0), Color::rgba(0.0, 0.0, 1.0, 1.0));
            let layer = cv
                .capture_layer(Size::new(10.0, 10.0), &mut |lc| {
                    lc.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::rgba(1.0, 0.0, 0.0, 1.0));
                })
                .expect("capture_layer 应支持");
            cv.draw_layer(&layer, Point::new(0.0, 0.0));
        }
        let mut surface = surface;
        // 层内(5,5)应为红，层外(20,20)应为蓝。
        let (r, _, _, _) = argb_at(&mut surface, 5, 5);
        assert!(r > 200, "blit 区应为红: r={r}");
        let (_, _, b, _) = argb_at(&mut surface, 20, 20);
        assert!(b > 200, "层外应为蓝: b={b}");
    }
}
