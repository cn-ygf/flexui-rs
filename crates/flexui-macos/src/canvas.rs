//! macOS 画布：用 AppKit 原生绘图原语（NSBezierPath / NSColor / NSString 绘制）
//! 实现平台无关的 `flexui_gfx::Canvas`。
//!
//! 这些原语都属 AppKit（系统框架），在 `NSView::drawRect:` 期间当前已有锁定的
//! 图形上下文，直接绘制即落到该视图上，符合「NSView 自绘」的要求。

use flexui_geometry::{Color, Corners, Point, Rect, Size};
use flexui_gfx::{Canvas, Font, ImageSource};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSBezierPath, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSGraphicsContext, NSImage, NSStringDrawing,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

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

    fn draw_image(&mut self, source: &ImageSource, rect: Rect) {
        let ImageSource::Path(p) = source;
        let ns_path = NSString::from_str(p);
        // 加载失败（文件不存在）时静默跳过，避免影响其它绘制。
        if let Some(img) = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path) {
            img.drawInRect(to_nsrect(rect));
        }
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
