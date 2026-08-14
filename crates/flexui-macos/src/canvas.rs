//! macOS 画布：用 AppKit 原生绘图原语（NSBezierPath / NSColor / NSString 绘制）
//! 实现平台无关的 `flexui_gfx::Canvas`。
//!
//! 这些原语都属 AppKit（系统框架），在 `NSView::drawRect:` 期间当前已有锁定的
//! 图形上下文，直接绘制即落到该视图上，符合「NSView 自绘」的要求。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use flexui_geometry::{pixel_aligned_stroke, Color, Corners, Insets, Point, Rect, Size};
use flexui_gfx::{Canvas, Font, ImageFit, ImageSource, TextBoundary, TextLayout};

use core_foundation::attributed_string::CFMutableAttributedString;
use core_foundation::base::{CFRange, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use core_graphics::context::CGContext;
use core_graphics::geometry::CGAffineTransform;
use core_text::font::{self as ct_font, CTFont};
use core_text::font_descriptor::{kCTFontBoldTrait, kCTFontItalicTrait};
use core_text::line::CTLine;
use core_text::string_attributes::{
    kCTFontAttributeName, kCTForegroundColorFromContextAttributeName,
};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSBezierPath, NSBitmapImageRep, NSColor, NSCompositingOperation, NSDeviceRGBColorSpace, NSFont,
    NSFontAttributeName, NSFontManager, NSFontTraitMask, NSForegroundColorAttributeName,
    NSGradient, NSGraphicsContext, NSImage, NSImageInterpolation, NSImageResizingMode,
    NSStringDrawing, NSUnderlineStyleAttributeName,
};
use objc2_foundation::{
    MainThreadMarker, NSData, NSDictionary, NSEdgeInsets, NSNumber, NSPoint, NSRect, NSSize,
    NSString,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ImageCacheKey {
    Path(String, u32),
    Bytes(usize, u32),
    Svg(usize, u32, u32, u32, u32),
}

struct CachedImage {
    image: Retained<NSImage>,
    // 保持 Arc 分配地址稳定，避免动态替换资源后缓存键被复用。
    _bytes: Option<Arc<Vec<u8>>>,
}

/// 缓存解码后的位图及按物理像素尺寸光栅化的 SVG。
#[derive(Default)]
pub(crate) struct ImageCache {
    images: HashMap<ImageCacheKey, CachedImage>,
}

pub(crate) type SharedImageCache = Rc<RefCell<ImageCache>>;

/// macOS 画布：绘制落到 drawRect 当前上下文，图片缓存由所属窗口共享。
pub struct CgCanvas {
    backing_scale: f32,
    image_cache: SharedImageCache,
}

impl CgCanvas {
    /// 构造 1× 独立画布，保持既有离屏绘制与测试调用兼容。
    pub fn new() -> Self {
        Self {
            backing_scale: 1.0,
            image_cache: Rc::new(RefCell::new(ImageCache::default())),
        }
    }

    pub(crate) fn with_image_cache(backing_scale: f32, image_cache: SharedImageCache) -> Self {
        Self {
            backing_scale: valid_scale(backing_scale),
            image_cache,
        }
    }

    /// 将描边中心路径收进原矩形，并对齐到物理像素网格。
    fn aligned_stroke(
        &self,
        rect: Rect,
        radius: Corners,
        line_width: f32,
    ) -> Option<(Rect, Corners, f32)> {
        let (path, aligned_width) = pixel_aligned_stroke(rect, line_width, self.backing_scale)?;
        let physical_width = aligned_width * self.backing_scale;
        let align_radius = |value: f32| {
            ((value.max(0.0) * self.backing_scale).round() - physical_width / 2.0).max(0.0)
                / self.backing_scale
        };
        Some((
            path,
            Corners {
                tl: align_radius(radius.tl),
                tr: align_radius(radius.tr),
                br: align_radius(radius.br),
                bl: align_radius(radius.bl),
            },
            aligned_width,
        ))
    }

    fn load_image(
        &self,
        source: &ImageSource,
        rect: Rect,
        fit: &ImageFit,
    ) -> Option<Retained<NSImage>> {
        match source {
            ImageSource::Path(path) => self.load_path(path, 1.0),
            ImageSource::ScaledPath(path, density) => self.load_path(path, *density),
            ImageSource::Bytes(bytes) => self.load_bytes(bytes, 1.0),
            ImageSource::ScaledBytes(bytes, density) => self.load_bytes(bytes, *density),
            ImageSource::Svg(bytes) => self.load_svg(bytes, rect, fit),
        }
    }

    fn load_path(&self, path: &str, density: f32) -> Option<Retained<NSImage>> {
        let density = valid_scale(density);
        let key = ImageCacheKey::Path(path.to_owned(), density.to_bits());
        if let Some(image) = self.cached(&key) {
            return Some(image);
        }
        let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path))?;
        if density != 1.0 {
            set_raster_logical_size(&image, density);
        }
        self.insert(key, image.clone(), None);
        Some(image)
    }

    fn load_bytes(&self, bytes: &Arc<Vec<u8>>, density: f32) -> Option<Retained<NSImage>> {
        let density = valid_scale(density);
        let key = ImageCacheKey::Bytes(Arc::as_ptr(bytes) as usize, density.to_bits());
        if let Some(image) = self.cached(&key) {
            return Some(image);
        }
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
        if density != 1.0 {
            set_raster_logical_size(&image, density);
        }
        self.insert(key, image.clone(), Some(bytes.clone()));
        Some(image)
    }

    fn load_svg(
        &self,
        bytes: &Arc<Vec<u8>>,
        rect: Rect,
        fit: &ImageFit,
    ) -> Option<Retained<NSImage>> {
        let logical = svg_logical_size(bytes, rect, fit);
        let pw = ((logical.width * self.backing_scale).round() as u32).max(1);
        let ph = ((logical.height * self.backing_scale).round() as u32).max(1);
        let key = ImageCacheKey::Svg(
            Arc::as_ptr(bytes) as usize,
            pw,
            ph,
            logical.width.to_bits(),
            logical.height.to_bits(),
        );
        if let Some(image) = self.cached(&key) {
            return Some(image);
        }
        let rgba = flexui_svg::rasterize(bytes, pw, ph)?;
        let image = unsafe { nsimage_from_rgba(&rgba, pw as usize, ph as usize, logical) }?;
        self.insert(key, image.clone(), Some(bytes.clone()));
        Some(image)
    }

    fn cached(&self, key: &ImageCacheKey) -> Option<Retained<NSImage>> {
        self.image_cache
            .borrow()
            .images
            .get(key)
            .map(|entry| entry.image.clone())
    }

    fn insert(&self, key: ImageCacheKey, image: Retained<NSImage>, bytes: Option<Arc<Vec<u8>>>) {
        self.image_cache.borrow_mut().images.insert(
            key,
            CachedImage {
                image,
                _bytes: bytes,
            },
        );
    }
}

impl Default for CgCanvas {
    fn default() -> Self {
        Self::new()
    }
}

fn valid_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn svg_logical_size(svg: &[u8], rect: Rect, fit: &ImageFit) -> Size {
    let fallback = Size::new(rect.size.width.max(1.0), rect.size.height.max(1.0));
    if matches!(fit, ImageFit::Stretch) {
        return fallback;
    }
    flexui_svg::intrinsic_size(svg)
        .map(|(width, height)| Size::new(width, height))
        .unwrap_or(fallback)
}

/// 把平台无关的 Rect 转成 AppKit 的 NSRect（坐标已是左上原点，视图设为 flipped）。
fn to_nsrect(r: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(r.origin.x as f64, r.origin.y as f64),
        NSSize::new(r.size.width as f64, r.size.height as f64),
    )
}

/// 从 RGBA(premultiplied) 字节建 NSImage（供 SVG 光栅化结果用）。
unsafe fn nsimage_from_rgba(
    rgba: &[u8],
    w: usize,
    h: usize,
    logical_size: Size,
) -> Option<Retained<NSImage>> {
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
    let logical = NSSize::new(logical_size.width as f64, logical_size.height as f64);
    rep.setSize(logical);
    let img = NSImage::initWithSize(NSImage::alloc(), logical);
    img.addRepresentation(&rep);
    Some(img)
}

/// AppKit 从 NSData 解码时不知道 `@2.00x` 的路径语义，需要显式设置 point 尺寸。
fn set_raster_logical_size(image: &NSImage, density: f32) {
    let reps = image.representations();
    let mut pixel_width = 0isize;
    let mut pixel_height = 0isize;
    for rep in &*reps {
        pixel_width = pixel_width.max(rep.pixelsWide());
        pixel_height = pixel_height.max(rep.pixelsHigh());
    }
    if pixel_width <= 0 || pixel_height <= 0 {
        return;
    }
    let logical = NSSize::new(
        pixel_width as f64 / density as f64,
        pixel_height as f64 / density as f64,
    );
    for rep in &*reps {
        rep.setSize(logical);
    }
    image.setSize(logical);
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
            draw_ninepatch(img, rect, *ins);
        }
    }
}

/// 使用 AppKit 原生 cap insets 一次绘制九宫格，避免九个切片分别插值时互相采样。
fn draw_ninepatch(img: &NSImage, rect: Rect, ins: Insets) {
    let old_insets = img.capInsets();
    let old_mode = img.resizingMode();
    img.setCapInsets(NSEdgeInsets {
        top: ins.top as f64,
        left: ins.left as f64,
        bottom: ins.bottom as f64,
        right: ins.right as f64,
    });
    img.setResizingMode(NSImageResizingMode::Stretch);
    img.drawInRect(to_nsrect(rect));
    img.setCapInsets(old_insets);
    img.setResizingMode(old_mode);
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
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(r, t),
        NSPoint::new(r, b),
        tr,
    );
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(r, b),
        NSPoint::new(l, b),
        br,
    );
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(l, b),
        NSPoint::new(l, t),
        bl,
    );
    path.appendBezierPathWithArcFromPoint_toPoint_radius(
        NSPoint::new(l, t),
        NSPoint::new(r, t),
        tl,
    );
    path.closePath();
    path
}

/// 把 Color 转成 NSColor（sRGB）。
fn to_nscolor(c: Color) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c.r as f64, c.g as f64, c.b as f64, c.a as f64)
}

/// 构造字体对象：有字族名走 fontWithName，否则系统字体；再按需叠加粗体/斜体特征。
fn to_nsfont(font: &Font) -> Retained<NSFont> {
    let base = if let Some(name) = &font.family {
        let ns_name = NSString::from_str(name);
        // fontWithName 可能返回 None（字体不存在），回退到系统字体。
        NSFont::fontWithName_size(&ns_name, font.size as f64)
            .unwrap_or_else(|| NSFont::systemFontOfSize(font.size as f64))
    } else {
        NSFont::systemFontOfSize(font.size as f64)
    };
    if !font.bold && !font.italic {
        return base;
    }
    // 经 NSFontManager 叠加粗体/斜体特征（不可用时保持原字体）。
    let Some(mtm) = MainThreadMarker::new() else {
        return base;
    };
    let mgr = NSFontManager::sharedFontManager(mtm);
    let mut f = base;
    if font.bold {
        f = mgr.convertFont_toHaveTrait(&f, NSFontTraitMask::BoldFontMask);
    }
    if font.italic {
        f = mgr.convertFont_toHaveTrait(&f, NSFontTraitMask::ItalicFontMask);
    }
    f
}

#[derive(Clone)]
struct CoreTextLayout {
    line: CTLine,
}

fn to_ctfont(font: &Font) -> CTFont {
    let base = font
        .family
        .as_deref()
        .and_then(|family| ct_font::new_from_name(family, font.size as f64).ok())
        .unwrap_or_else(|| {
            ct_font::new_ui_font_for_language(
                ct_font::kCTFontSystemFontType,
                font.size as f64,
                None,
            )
        });
    let mut traits = 0;
    if font.bold {
        traits |= kCTFontBoldTrait;
    }
    if font.italic {
        traits |= kCTFontItalicTrait;
    }
    if traits == 0 {
        base
    } else {
        base.clone_with_symbolic_traits(traits, traits)
            .unwrap_or(base)
    }
}

fn core_text_layout(text: &str, font: &Font) -> TextLayout {
    let ct_font = to_ctfont(font);
    let mut attributed = CFMutableAttributedString::new();
    let string = CFString::new(text);
    attributed.replace_str(&string, CFRange::init(0, 0));
    let range = CFRange::init(0, attributed.char_len());
    attributed.set_attribute(range, unsafe { kCTFontAttributeName }, &ct_font);
    attributed.set_attribute(
        range,
        unsafe { kCTForegroundColorFromContextAttributeName },
        &CFBoolean::true_value(),
    );
    let line = CTLine::new_with_attributed_string(attributed.as_concrete_TypeRef());
    let bounds = line.get_typographic_bounds();
    let ascent = (bounds.ascent as f32).max(ct_font.ascent() as f32);
    let descent = (bounds.descent as f32).max(ct_font.descent() as f32);
    let height = (ascent + descent).max(font.size);

    let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
    boundaries.push(TextBoundary {
        char_index: 0,
        x: line.get_string_offset_for_string_index(0) as f32,
    });
    let mut utf16_index = 0usize;
    for (char_index, ch) in text.chars().enumerate() {
        utf16_index += ch.len_utf16();
        boundaries.push(TextBoundary {
            char_index: char_index + 1,
            x: line.get_string_offset_for_string_index(utf16_index as isize) as f32,
        });
    }

    TextLayout::new(
        text,
        font.clone(),
        Size::new(bounds.width as f32, height),
        ascent,
        descent,
        boundaries,
    )
    .with_platform_data(Rc::new(CoreTextLayout { line }))
}

/// 构造文字属性字典：字体 + 前景色（+ 下划线）。
fn text_attributes(font: &Font, color: Color) -> Retained<NSDictionary<NSString, AnyObject>> {
    let ns_font = to_nsfont(font);
    let ns_color = to_nscolor(color);
    // 属性名是 AppKit 的 extern 静态量，读取需 unsafe。
    let (k_font, k_color) = unsafe { (NSFontAttributeName, NSForegroundColorAttributeName) };
    let font_obj: &AnyObject = &ns_font;
    let color_obj: &AnyObject = &ns_color;
    let mut keys: Vec<&NSString> = vec![k_font, k_color];
    let mut vals: Vec<&AnyObject> = vec![font_obj, color_obj];
    // 下划线：NSUnderlineStyleSingle = 1。
    let underline = NSNumber::numberWithInt(1);
    if font.underline {
        let k_underline = unsafe { NSUnderlineStyleAttributeName };
        let u_obj: &AnyObject = &underline;
        keys.push(k_underline);
        vals.push(u_obj);
    }
    NSDictionary::from_slices(&keys, &vals)
}

impl Canvas for CgCanvas {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        to_nscolor(color).set();
        NSBezierPath::fillRect(to_nsrect(rect));
    }

    fn stroke_rect(&mut self, rect: Rect, color: Color, line_width: f32) {
        let Some((path_rect, _, line_width)) =
            self.aligned_stroke(rect, Corners::default(), line_width)
        else {
            return;
        };
        let path = NSBezierPath::bezierPathWithRect(to_nsrect(path_rect));
        path.setLineWidth(line_width as f64);
        to_nscolor(color).set();
        path.stroke();
    }

    fn fill_round_rect(&mut self, rect: Rect, radius: Corners, color: Color) {
        let path = round_rect_path(rect, radius);
        to_nscolor(color).set();
        path.fill();
    }

    fn fill_gradient_rect(
        &mut self,
        rect: Rect,
        radius: Corners,
        from: Color,
        to: Color,
        vertical: bool,
    ) {
        let path = round_rect_path(rect, radius);
        let grad = NSGradient::initWithStartingColor_endingColor(
            NSGradient::alloc(),
            &to_nscolor(from),
            &to_nscolor(to),
        );
        if let Some(grad) = grad {
            // 视图为 flipped（y 向下）：竖直=90°（from 在上→to 在下），水平=0°（左→右）。
            let angle = if vertical { 90.0 } else { 0.0 };
            grad.drawInBezierPath_angle(&path, angle);
        } else {
            to_nscolor(from).set();
            path.fill();
        }
    }

    fn stroke_round_rect(&mut self, rect: Rect, radius: Corners, color: Color, line_width: f32) {
        let Some((path_rect, path_radius, line_width)) =
            self.aligned_stroke(rect, radius, line_width)
        else {
            return;
        };
        let path = round_rect_path(path_rect, path_radius);
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

    fn layout_text(&self, text: &str, font: &Font) -> TextLayout {
        core_text_layout(text, font)
    }

    fn draw_text_layout(&mut self, layout: &TextLayout, origin: Point, color: Color) {
        let Some(data) = layout.platform_data::<CoreTextLayout>() else {
            self.draw_text_advance(layout.text(), origin, layout.font(), color);
            return;
        };
        let Some(ns_context) = NSGraphicsContext::currentContext() else {
            return;
        };
        let ns_cg = ns_context.CGContext();
        let raw = Retained::as_ptr(&ns_cg).cast_mut().cast();
        let cg = unsafe { CGContext::from_existing_context_ptr(raw) };
        cg.save();
        cg.set_rgb_fill_color(
            color.r as f64,
            color.g as f64,
            color.b as f64,
            color.a as f64,
        );
        cg.set_text_matrix(&CGAffineTransform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
        cg.translate(origin.x as f64, (origin.y + layout.ascent()) as f64);
        cg.scale(1.0, -1.0);
        cg.set_text_position(0.0, 0.0);
        data.line.draw(&cg);
        cg.restore();

        if layout.font().underline {
            self.fill_rect(
                Rect::new(
                    origin.x,
                    origin.y + layout.ascent() + layout.descent() - 1.0,
                    layout.width(),
                    1.0,
                ),
                color,
            );
        }
    }

    fn draw_image(&mut self, source: &ImageSource, rect: Rect, tint: Option<Color>, fit: ImageFit) {
        // 加载失败时静默跳过，避免影响其它绘制。
        let Some(img) = self.load_image(source, rect, &fit) else {
            return;
        };
        // tint 换色（保留 alpha 形状）。
        let img = match tint {
            Some(c) => tinted_image(&img, c),
            None => img,
        };
        NSGraphicsContext::saveGraphicsState_class();
        if let Some(ctx) = NSGraphicsContext::currentContext() {
            ctx.setImageInterpolation(NSImageInterpolation::High);
        }
        draw_nsimage(&img, rect, &fit);
        NSGraphicsContext::restoreGraphicsState_class();
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

    fn clip_round_rect(&mut self, rect: Rect, radius: Corners) {
        round_rect_path(rect, radius).addClip();
    }
}

/// 度量文字时颜色不影响尺寸，用黑色占位。
fn color_black() -> Color {
    Color::BLACK
}

#[cfg(test)]
mod text_tests {
    use super::*;

    #[test]
    fn retina圆角描边完整落在原矩形内() {
        let canvas = CgCanvas {
            backing_scale: 2.0,
            image_cache: Rc::new(RefCell::new(ImageCache::default())),
        };
        let (path, radius, width) = canvas
            .aligned_stroke(Rect::new(10.0, 20.0, 16.0, 16.0), Corners::all(8.0), 1.5)
            .unwrap();

        assert_eq!(path, Rect::new(10.75, 20.75, 14.5, 14.5));
        assert_eq!(radius, Corners::all(7.25));
        assert_eq!(width, 1.5);
        assert_eq!(path.left() - width / 2.0, 10.0);
        assert_eq!(path.right() + width / 2.0, 26.0);
    }

    #[test]
    fn coretext追加字符不改变既有普通字符边界() {
        let canvas = CgCanvas::new();
        let font = Font::system(16.0);
        let before = canvas.layout_text("Flex界面", &font);
        let after = canvas.layout_text("Flex界面Z", &font);
        assert_eq!(before.boundaries().len() + 1, after.boundaries().len());
        for (a, b) in before.boundaries().iter().zip(after.boundaries()) {
            assert!(
                (a.x - b.x).abs() < 0.01,
                "边界 {} 发生漂移: {} -> {}",
                a.char_index,
                a.x,
                b.x
            );
        }
    }
}
