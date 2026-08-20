//! 用系统图形接口 Cairo(2D) + Pango(文字) 实现 `flexui_gfx::Canvas`。
//!
//! 渲染目标是内存中的 `cairo::ImageSurface`（ARGB32，物理像素）。窗口层每帧把它的
//! 像素 blit 到 X11 窗口。坐标一律「逻辑点、左上原点、y 向下」——构造时对 context
//! 施加 `scale`，之后所有绘制都用逻辑坐标；Cairo 原生支持 save/restore/clip。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cairo::{Context, Filter, Format, ImageSurface, LinearGradient, SurfacePattern};
use flexui_gfx::{
    Canvas, Color, Corners, Font, ImageFit, ImageSource, Insets, LayerHandle, Point, Rect, Size,
    TextBoundary, TextLayout,
};

/// 解码后位图的缓存键。
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum ImageKey {
    /// 位图：路径 或 字节缓冲地址（按源缓存，绘制时再缩放）。
    Path(String),
    Bytes(usize),
    /// SVG：按源地址 + 目标物理尺寸缓存（尺寸变了要重光栅）。
    Svg(usize, u32, u32),
}

/// 缓存的位图：cairo surface + 像素密度（物理像素/逻辑点）。
pub(crate) struct CachedImage {
    surface: ImageSurface,
    density: f32,
}

/// 跨帧共享的图片缓存（避免每帧重复解码/光栅）。
pub(crate) type SharedImageCache = Rc<RefCell<HashMap<ImageKey, CachedImage>>>;

/// 建一个空的共享图片缓存（窗口持有、每帧复用）。
pub(crate) fn new_image_cache() -> SharedImageCache {
    Rc::new(RefCell::new(HashMap::new()))
}

/// Cairo 画布：持有绑定到目标 surface 的 context（引用计数，无需生命周期）。
pub struct CairoCanvas {
    cr: Context,
    /// 像素密度（HiDPI）。绘制用逻辑坐标，context 已按此缩放。
    scale: f32,
    images: SharedImageCache,
}

impl CairoCanvas {
    /// 用一块 ARGB32 ImageSurface 建画布（自带独立图片缓存，供测试/离屏）。
    pub fn new(surface: &ImageSurface, scale: f32) -> Self {
        Self::with_images(surface, scale, Rc::new(RefCell::new(HashMap::new())))
    }

    /// 用共享图片缓存建画布（窗口每帧复用同一份缓存）。
    pub(crate) fn with_images(surface: &ImageSurface, scale: f32, images: SharedImageCache) -> Self {
        let cr = Context::new(surface).expect("cairo context");
        let s = scale.max(0.01) as f64;
        cr.scale(s, s);
        Self { cr, scale, images }
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

    /// 把整块 surface 清为透明（每帧重绘前调用，避免半透明叠加内容残影）。
    pub(crate) fn clear(&mut self) {
        let _ = self.cr.save();
        self.cr.set_operator(cairo::Operator::Clear);
        let _ = self.cr.paint();
        self.cr.set_operator(cairo::Operator::Over);
        let _ = self.cr.restore();
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

    /// 取源图对应的 cairo surface（含缓存 + 解码/光栅），返回 (surface, density)。
    fn image_surface(&self, source: &ImageSource, rect: Rect) -> Option<(ImageSurface, f32)> {
        use std::sync::Arc;
        let svg_size = || {
            let pw = (rect.size.width * self.scale).round().max(1.0) as u32;
            let ph = (rect.size.height * self.scale).round().max(1.0) as u32;
            (pw, ph)
        };
        let key = match source {
            ImageSource::Path(p) | ImageSource::ScaledPath(p, _) => ImageKey::Path(p.clone()),
            ImageSource::Bytes(b) | ImageSource::ScaledBytes(b, _) => {
                ImageKey::Bytes(Arc::as_ptr(b) as *const u8 as usize)
            }
            ImageSource::Svg(b) => {
                let (pw, ph) = svg_size();
                ImageKey::Svg(Arc::as_ptr(b) as *const u8 as usize, pw, ph)
            }
        };
        if let Some(c) = self.images.borrow().get(&key) {
            return Some((c.surface.clone(), c.density));
        }
        let (surface, density) = match source {
            ImageSource::Path(p) => (decode_path(p)?, 1.0),
            ImageSource::ScaledPath(p, d) => (decode_path(p)?, *d),
            ImageSource::Bytes(b) => (decode_bytes(b)?, 1.0),
            ImageSource::ScaledBytes(b, d) => (decode_bytes(b)?, *d),
            ImageSource::Svg(b) => {
                let (pw, ph) = svg_size();
                let rgba = flexui_svg::rasterize(b, pw, ph)?;
                (surface_from_premul_rgba(&rgba, pw, ph)?, self.scale)
            }
        };
        self.images.borrow_mut().insert(
            key,
            CachedImage {
                surface: surface.clone(),
                density,
            },
        );
        Some((surface, density))
    }

    /// 把源图的像素子区 `(sx0,sy0,spw,sph)` 缩放绘制到逻辑目标矩形 `dst`。
    /// `tint` 为 Some 时用其色 + 源 alpha 重着色（黑图换色）。
    fn blit_sub(
        &self,
        surface: &ImageSurface,
        src: (f64, f64, f64, f64),
        dst: Rect,
        tint: Option<Color>,
    ) {
        let (sx0, sy0, spw, sph) = src;
        if spw <= 0.0 || sph <= 0.0 || dst.size.width <= 0.0 || dst.size.height <= 0.0 {
            return;
        }
        self.cr.save().ok();
        self.cr.rectangle(
            dst.left() as f64,
            dst.top() as f64,
            dst.size.width as f64,
            dst.size.height as f64,
        );
        self.cr.clip();
        self.cr.translate(dst.left() as f64, dst.top() as f64);
        self.cr.scale(dst.size.width as f64 / spw, dst.size.height as f64 / sph);
        if let Some(c) = tint {
            self.cr
                .set_source_rgba(c.r as f64, c.g as f64, c.b as f64, c.a as f64);
            self.cr.mask_surface(surface, -sx0, -sy0).ok();
        } else {
            self.cr.set_source_surface(surface, -sx0, -sy0).ok();
            self.cr.source().set_filter(Filter::Good);
            self.cr.paint().ok();
        }
        self.cr.restore().ok();
    }

    /// 平铺绘制（按 density 缩回逻辑尺寸重复）。
    fn tile(&self, surface: &ImageSurface, rect: Rect, density: f32, _tint: Option<Color>) {
        let inv = 1.0 / density.max(0.01) as f64;
        self.cr.save().ok();
        self.cr.translate(rect.left() as f64, rect.top() as f64);
        self.cr.scale(inv, inv);
        let pattern = SurfacePattern::create(surface);
        pattern.set_extend(cairo::Extend::Repeat);
        pattern.set_filter(Filter::Good);
        self.cr.set_source(&pattern).ok();
        self.cr.rectangle(
            0.0,
            0.0,
            rect.size.width as f64 / inv,
            rect.size.height as f64 / inv,
        );
        self.cr.fill().ok();
        self.cr.restore().ok();
    }

    /// 九宫格：四角原尺寸、四边与中间拉伸。`ins` 为源图四边不拉伸边距（逻辑点）。
    fn nine_patch(
        &self,
        surface: &ImageSurface,
        rect: Rect,
        density: f32,
        ins: Insets,
        tint: Option<Color>,
    ) {
        let sw = surface.width() as f64;
        let sh = surface.height() as f64;
        let d = density.max(0.01) as f64;
        // 源图三段（像素）。
        let (sl, st, sr, sb) = (
            ins.left as f64 * d,
            ins.top as f64 * d,
            ins.right as f64 * d,
            ins.bottom as f64 * d,
        );
        let smid_w = (sw - sl - sr).max(0.0);
        let smid_h = (sh - st - sb).max(0.0);
        // 目标三段（逻辑点）：角保持逻辑尺寸(ins)，中间拉伸。
        let (dl, dt, dr, db) = (
            ins.left,
            ins.top,
            ins.right,
            ins.bottom,
        );
        let dmid_w = (rect.size.width - dl - dr).max(0.0);
        let dmid_h = (rect.size.height - dt - db).max(0.0);
        let x0 = rect.left();
        let x1 = rect.left() + dl;
        let x2 = rect.right() - dr;
        let y0 = rect.top();
        let y1 = rect.top() + dt;
        let y2 = rect.bottom() - db;
        // 9 段：(源像素矩形, 目标逻辑矩形)。
        let cells = [
            ((0.0, 0.0, sl, st), Rect::new(x0, y0, dl, dt)),
            ((sl, 0.0, smid_w, st), Rect::new(x1, y0, dmid_w, dt)),
            ((sw - sr, 0.0, sr, st), Rect::new(x2, y0, dr, dt)),
            ((0.0, st, sl, smid_h), Rect::new(x0, y1, dl, dmid_h)),
            ((sl, st, smid_w, smid_h), Rect::new(x1, y1, dmid_w, dmid_h)),
            ((sw - sr, st, sr, smid_h), Rect::new(x2, y1, dr, dmid_h)),
            ((0.0, sh - sb, sl, sb), Rect::new(x0, y2, dl, db)),
            ((sl, sh - sb, smid_w, sb), Rect::new(x1, y2, dmid_w, db)),
            ((sw - sr, sh - sb, sr, sb), Rect::new(x2, y2, dr, db)),
        ];
        for (s, d) in cells {
            self.blit_sub(surface, s, d, tint);
        }
    }
}

/// 解码磁盘位图为 cairo surface（image crate 解码 → ARGB32 预乘）。
fn decode_path(path: &str) -> Option<ImageSurface> {
    let img = image::open(path).ok()?.to_rgba8();
    surface_from_straight_rgba(img.as_raw(), img.width(), img.height())
}

/// 解码内存位图为 cairo surface。
fn decode_bytes(bytes: &[u8]) -> Option<ImageSurface> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    surface_from_straight_rgba(img.as_raw(), img.width(), img.height())
}

/// 非预乘 RGBA → cairo ARGB32（小端 BGRA、预乘）。
fn surface_from_straight_rgba(rgba: &[u8], w: u32, h: u32) -> Option<ImageSurface> {
    let stride = (w * 4) as usize;
    let mut buf = vec![0u8; stride * h as usize];
    for i in 0..(w * h) as usize {
        let o = i * 4;
        let (r, g, b, a) = (rgba[o] as u16, rgba[o + 1] as u16, rgba[o + 2] as u16, rgba[o + 3]);
        let a16 = a as u16;
        buf[o] = (b * a16 / 255) as u8;
        buf[o + 1] = (g * a16 / 255) as u8;
        buf[o + 2] = (r * a16 / 255) as u8;
        buf[o + 3] = a;
    }
    ImageSurface::create_for_data(buf, Format::ARgb32, w as i32, h as i32, stride as i32).ok()
}

/// 预乘 RGBA（SVG 光栅结果）→ cairo ARGB32（仅 R/B 互换）。
fn surface_from_premul_rgba(rgba: &[u8], w: u32, h: u32) -> Option<ImageSurface> {
    let stride = (w * 4) as usize;
    let mut buf = vec![0u8; stride * h as usize];
    for i in 0..(w * h) as usize {
        let o = i * 4;
        buf[o] = rgba[o + 2];
        buf[o + 1] = rgba[o + 1];
        buf[o + 2] = rgba[o];
        buf[o + 3] = rgba[o + 3];
    }
    ImageSurface::create_for_data(buf, Format::ARgb32, w as i32, h as i32, stride as i32).ok()
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
        source: &ImageSource,
        rect: Rect,
        tint: Option<Color>,
        fit: ImageFit,
    ) {
        if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
            return;
        }
        let Some((surface, density)) = self.image_surface(source, rect) else {
            return;
        };
        let sw = surface.width() as f64;
        let sh = surface.height() as f64;
        if sw <= 0.0 || sh <= 0.0 {
            return;
        }
        // 逻辑源尺寸（位图按 density 缩回逻辑点）。
        let lw = sw / density as f64;
        let lh = sh / density as f64;

        self.cr.save().ok();
        // 裁到目标矩形，避免 Center/Tile/NinePatch 溢出。
        self.cr.rectangle(
            rect.left() as f64,
            rect.top() as f64,
            rect.size.width as f64,
            rect.size.height as f64,
        );
        self.cr.clip();

        match fit {
            ImageFit::Stretch => {
                self.blit_sub(&surface, (0.0, 0.0, sw, sh), rect, tint);
            }
            ImageFit::Center => {
                let x = rect.left() as f32 + (rect.size.width - lw as f32) / 2.0;
                let y = rect.top() as f32 + (rect.size.height - lh as f32) / 2.0;
                self.blit_sub(&surface, (0.0, 0.0, sw, sh), Rect::new(x, y, lw as f32, lh as f32), tint);
            }
            ImageFit::Tile => {
                self.tile(&surface, rect, density, tint);
            }
            ImageFit::NinePatch(ins) => {
                self.nine_patch(&surface, rect, density, ins, tint);
            }
        }
        self.cr.restore().ok();
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
            let mut off = CairoCanvas::with_images(&surface, self.scale, self.images.clone());
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
