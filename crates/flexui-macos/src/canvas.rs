//! macOS 画布：用 AppKit 原生绘图原语（NSBezierPath / NSColor / NSString 绘制）
//! 实现平台无关的 `flexui_gfx::Canvas`。
//!
//! 这些原语都属 AppKit（系统框架），在 `NSView::drawRect:` 期间当前已有锁定的
//! 图形上下文，直接绘制即落到该视图上，符合「NSView 自绘」的要求。

use flexui_geometry::{Color, Corners, Insets, Point, Rect, Size};
use flexui_gfx::{Canvas, Font, ImageFit, ImageSource};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSBezierPath, NSBitmapImageRep, NSColor, NSCompositingOperation, NSDeviceRGBColorSpace, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSGraphicsContext, NSImage, NSStringDrawing,
};
use objc2_foundation::{NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString};

/// macOS 画布：无内部状态，绘制落到 drawRect 当前上下文。
pub struct CgCanvas;

impl CgCanvas {
    pub fn new() -> Self {
        CgCanvas
    }
}

impl Default for CgCanvas {
    fn default() -> Self {
        Self::new()
    }
}

/// 把平台无关的 Rect 转成 AppKit 的 NSRect（坐标已是左上原点，视图设为 flipped）。
fn to_nsrect(r: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(r.origin.x as f64, r.origin.y as f64),
        NSSize::new(r.size.width as f64, r.size.height as f64),
    )
}

/// 从 RGBA(premultiplied) 字节建 NSImage（供 SVG 光栅化结果用）。
unsafe fn nsimage_from_rgba(rgba: &[u8], w: usize, h: usize) -> Option<Retained<NSImage>> {
    // planes 传 null → NSBitmapImageRep 自行分配缓冲，再把 RGBA 拷进去（避免生命周期问题）。
    let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
        NSBitmapImageRep::alloc(),
        std::ptr::null_mut(),
        w as isize,
        h as isize,
        8,
        4,
        true,
        false,
        NSDeviceRGBColorSpace,
        (w * 4) as isize,
        32,
    )?;
    let dst = rep.bitmapData();
    if dst.is_null() {
        return None;
    }
    std::ptr::copy_nonoverlapping(rgba.as_ptr(), dst, w * h * 4);
    let img = NSImage::initWithSize(NSImage::alloc(), NSSize::new(w as f64, h as f64));
    img.addRepresentation(&rep);
    Some(img)
}

/// 生成 tint 换色后的 NSImage：原图 + SourceAtop 目标色填充（保留 alpha 形状）。
#[allow(deprecated)] // lockFocus 已弃用但对位图 tint 足够可靠
fn tinted_image(img: &NSImage, color: Color) -> Retained<NSImage> {
    let size = img.size();
    let bounds = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let out = NSImage::initWithSize(NSImage::alloc(), size);
    out.lockFocus();
    img.drawInRect_fromRect_operation_fraction(
        bounds,
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
        NSCompositingOperation::SourceOver,
        1.0,
    );
    if let Some(ctx) = NSGraphicsContext::currentContext() {
        ctx.setCompositingOperation(NSCompositingOperation::SourceAtop);
    }
    to_nscolor(color).set();
    NSBezierPath::fillRect(bounds);
    out.unlockFocus();
    out
}

/// 按 fit 绘制 NSImage 到目标矩形（坐标为 flipped 视图的左上原点）。
fn draw_nsimage(img: &NSImage, rect: Rect, fit: &ImageFit) {
    let size = img.size();
    let (iw, ih) = (size.width as f32, size.height as f32);
    let full_src = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
    let op = NSCompositingOperation::SourceOver;
    match fit {
        ImageFit::Stretch => img.drawInRect(to_nsrect(rect)),
        ImageFit::Center => {
            let x = rect.left() + (rect.size.width - iw) / 2.0;
            let y = rect.top() + (rect.size.height - ih) / 2.0;
            img.drawInRect(to_nsrect(Rect::new(x, y, iw, ih)));
        }
        ImageFit::Tile => {
            let mut y = rect.top();
            while y < rect.bottom() {
                let mut x = rect.left();
                while x < rect.right() {
                    img.drawInRect(to_nsrect(Rect::new(x, y, iw, ih)));
                    x += iw;
                }
                y += ih;
            }
        }
        ImageFit::NinePatch(ins) => {
            draw_ninepatch(img, rect, *ins, iw, ih, full_src, op);
        }
    }
}

/// 九宫格绘制（源图坐标为 NSImage 左下原点，故行按底部映射）。
#[allow(clippy::too_many_arguments)]
fn draw_ninepatch(
    img: &NSImage,
    rect: Rect,
    ins: Insets,
    iw: f32,
    ih: f32,
    _full: NSRect,
    op: NSCompositingOperation,
) {
    // 目标 3x3 边界（左上原点）。
    let cd = [rect.left(), rect.left() + ins.left, rect.right() - ins.right, rect.right()];
    let rd = [rect.top(), rect.top() + ins.top, rect.bottom() - ins.bottom, rect.bottom()];
    // 源列边界（左→右）。
    let cs = [0.0, ins.left, iw - ins.right, iw];
    // 源行边界（NSImage 底部原点；视觉 top→bottom 对应源 y 从高到低）。
    let rs_bottom = [ih, ih - ins.top, ins.bottom, 0.0]; // 每视觉行的顶边（源 y）
    for r in 0..3usize {
        for c in 0..3usize {
            let dx = cd[c];
            let dw = cd[c + 1] - cd[c];
            let dy = rd[r];
            let dh = rd[r + 1] - rd[r];
            let sx = cs[c];
            let sw = cs[c + 1] - cs[c];
            let s_top = rs_bottom[r];
            let s_h = s_top - rs_bottom[r + 1];
            let s_y = s_top - s_h;
            if dw > 0.0 && dh > 0.0 && sw > 0.0 && s_h > 0.0 {
                img.drawInRect_fromRect_operation_fraction(
                    to_nsrect(Rect::new(dx, dy, dw, dh)),
                    NSRect::new(NSPoint::new(sx as f64, s_y as f64), NSSize::new(sw as f64, s_h as f64)),
                    op,
                    1.0,
                );
            }
        }
    }
}

/// 构造四角独立圆角的矩形路径（用 arcTo 逐角连接）。
fn round_rect_path(rect: Rect, radius: Corners) -> Retained<NSBezierPath> {
    let l = rect.left() as f64;
    let t = rect.top() as f64;
    let r = rect.right() as f64;
    let b = rect.bottom() as f64;
    let hw = (rect.size.width / 2.0) as f64;
    let hh = (rect.size.height / 2.0) as f64;
    let clamp = |v: f32| (v as f64).max(0.0).min(hw).min(hh);
    let tl = clamp(radius.tl);
    let tr = clamp(radius.tr);
    let br = clamp(radius.br);
    let bl = clamp(radius.bl);

    let path = NSBezierPath::bezierPath();
    path.moveToPoint(NSPoint::new(l + tl, t));
    // 顶边 → 右上角 → 右边 → 右下角 → 底边 → 左下角 → 左边 → 左上角
    path.appendBezierPathWithArcFromPoint_toPoint_radius(NSPoint::new(r, t), NSPoint::new(r, b), tr);
    path.appendBezierPathWithArcFromPoint_toPoint_radius(NSPoint::new(r, b), NSPoint::new(l, b), br);
    path.appendBezierPathWithArcFromPoint_toPoint_radius(NSPoint::new(l, b), NSPoint::new(l, t), bl);
    path.appendBezierPathWithArcFromPoint_toPoint_radius(NSPoint::new(l, t), NSPoint::new(r, t), tl);
    path.closePath();
    path
}

/// 把 Color 转成 NSColor（sRGB）。
fn to_nscolor(c: Color) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c.r as f64, c.g as f64, c.b as f64, c.a as f64)
}

/// 构造字体对象：有字族名走 fontWithName，否则用系统字体。
fn to_nsfont(font: &Font) -> Retained<NSFont> {
    if let Some(name) = &font.family {
        let ns_name = NSString::from_str(name);
        // fontWithName 可能返回 None（字体不存在），回退到系统字体。
        NSFont::fontWithName_size(&ns_name, font.size as f64)
            .unwrap_or_else(|| NSFont::systemFontOfSize(font.size as f64))
    } else {
        NSFont::systemFontOfSize(font.size as f64)
    }
}

/// 构造文字属性字典：字体 + 前景色。
fn text_attributes(
    font: &Font,
    color: Color,
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let ns_font = to_nsfont(font);
    let ns_color = to_nscolor(color);
    // 通过 Deref 链把具体对象强制成 &AnyObject 作为字典的值。
    let font_obj: &AnyObject = &ns_font;
    let color_obj: &AnyObject = &ns_color;
    // 属性名是 AppKit 的 extern 静态量，读取需 unsafe。
    let (k_font, k_color) =
        unsafe { (NSFontAttributeName, NSForegroundColorAttributeName) };
    NSDictionary::from_slices(&[k_font, k_color], &[font_obj, color_obj])
}

impl Canvas for CgCanvas {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        to_nscolor(color).set();
        NSBezierPath::fillRect(to_nsrect(rect));
    }

    fn stroke_rect(&mut self, rect: Rect, color: Color, line_width: f32) {
        let path = NSBezierPath::bezierPathWithRect(to_nsrect(rect));
        path.setLineWidth(line_width as f64);
        to_nscolor(color).set();
        path.stroke();
    }

    fn fill_round_rect(&mut self, rect: Rect, radius: Corners, color: Color) {
        let path = round_rect_path(rect, radius);
        to_nscolor(color).set();
        path.fill();
    }

    fn stroke_round_rect(&mut self, rect: Rect, radius: Corners, color: Color, line_width: f32) {
        let path = round_rect_path(rect, radius);
        path.setLineWidth(line_width as f64);
        to_nscolor(color).set();
        path.stroke();
    }

    fn draw_text(&mut self, text: &str, origin: Point, font: &Font, color: Color) {
        let ns_text = NSString::from_str(text);
        let attrs = text_attributes(font, color);
        // drawAtPoint 在 flipped 视图中以 point 作为文字左上角。
        unsafe {
            ns_text.drawAtPoint_withAttributes(
                NSPoint::new(origin.x as f64, origin.y as f64),
                Some(&attrs),
            );
        }
    }

    fn measure_text(&self, text: &str, font: &Font) -> Size {
        let ns_text = NSString::from_str(text);
        let attrs = text_attributes(font, color_black());
        let sz: NSSize = unsafe { ns_text.sizeWithAttributes(Some(&attrs)) };
        Size::new(sz.width as f32, sz.height as f32)
    }

    fn draw_image(&mut self, source: &ImageSource, rect: Rect, tint: Option<Color>, fit: ImageFit) {
        // 加载失败时静默跳过，避免影响其它绘制。
        let img = match source {
            ImageSource::Path(p) => {
                NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(p))
            }
            ImageSource::Bytes(b) => {
                NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(b))
            }
            ImageSource::Svg(b) => {
                // 2× 超采样光栅化 → RGBA → NSImage。
                let pw = ((rect.size.width * 2.0).round() as u32).max(1);
                let ph = ((rect.size.height * 2.0).round() as u32).max(1);
                flexui_svg::rasterize(b, pw, ph)
                    .and_then(|rgba| unsafe { nsimage_from_rgba(&rgba, pw as usize, ph as usize) })
            }
        };
        let Some(img) = img else { return };
        // tint 换色（保留 alpha 形状）。
        let img = match tint {
            Some(c) => tinted_image(&img, c),
            None => img,
        };
        draw_nsimage(&img, rect, &fit);
    }

    fn save(&mut self) {
        NSGraphicsContext::saveGraphicsState_class();
    }

    fn restore(&mut self) {
        NSGraphicsContext::restoreGraphicsState_class();
    }

    fn clip_rect(&mut self, rect: Rect) {
        // 追加矩形裁剪区（配合 save/restore 使用）。
        NSBezierPath::bezierPathWithRect(to_nsrect(rect)).addClip();
    }
}

/// 度量文字时颜色不影响尺寸，用黑色占位。
fn color_black() -> Color {
    Color::BLACK
}
