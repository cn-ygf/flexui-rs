//! Windows 画布：用 GDI+ flat API 实现平台无关的 `flexui_gfx::Canvas`。
//!
//! 与 macOS 的 CgCanvas 对位。坐标为逻辑像素、左上原点，与统一坐标系一致。

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

use flexui_geometry::{pixel_aligned_stroke, Color, Corners, Point, Rect, Size};
use flexui_gfx::{Canvas, Font, ImageFit, ImageSource, TextLayout};
use windows_sys::Win32::Graphics::GdiPlus as gp;
use windows_sys::Win32::UI::Shell::SHCreateMemStream;

use crate::gdiplus::{
    COMBINE_INTERSECT, FILLMODE_ALTERNATE, INTERPOLATION_HIGH_QUALITY_BICUBIC,
    MATRIX_ORDER_PREPEND, PIXEL_OFFSET_HIGH_QUALITY, SMOOTHING_ANTIALIAS, TEXT_HINT_CLEARTYPE,
    UNIT_PIXEL,
};

/// Windows 默认 UI 字体的英文字族名（微软雅黑）。
const DEFAULT_FONT_FAMILY: &str = "Microsoft YaHei";

/// GDI+ 画布，持有一个 Graphics 指针（来自窗口 HDC 或离屏位图）。
pub struct GdiCanvas<'a> {
    g: *mut gp::GpGraphics,
    saved: Vec<u32>,
    saved_clips: Vec<Option<Rect>>,
    clip: Option<Rect>,
    dpi_scale: f32,
    image_cache: Option<&'a mut ImageCache>,
}

impl GdiCanvas<'_> {
    /// 用给定 Graphics 构造，并开启抗锯齿与清晰文字。
    pub fn new(g: *mut gp::GpGraphics) -> Self {
        unsafe {
            gp::GdipSetSmoothingMode(g, SMOOTHING_ANTIALIAS);
            gp::GdipSetInterpolationMode(g, INTERPOLATION_HIGH_QUALITY_BICUBIC);
            gp::GdipSetPixelOffsetMode(g, PIXEL_OFFSET_HIGH_QUALITY);
            gp::GdipSetTextRenderingHint(g, TEXT_HINT_CLEARTYPE);
        }
        Self {
            g,
            saved: Vec::new(),
            saved_clips: Vec::new(),
            clip: None,
            dpi_scale: 1.0,
            image_cache: None,
        }
    }

    /// 使用窗口级图片缓存，避免每帧重新解码或光栅化。
    pub fn with_cache(g: *mut gp::GpGraphics, cache: &mut ImageCache) -> GdiCanvas<'_> {
        let mut canvas = GdiCanvas::new(g);
        canvas.image_cache = Some(cache);
        canvas
    }

    /// 用不透明底色清屏（保证 ClearType 文字有实底、blit 无残留）。
    pub fn clear(&mut self, color: Color) {
        unsafe { gp::GdipGraphicsClear(self.g, argb(color)) };
    }

    /// 施加 DPI 缩放：之后所有逻辑坐标按 scale 放大到物理像素（HiDPI 清晰）。
    pub fn set_dpi_scale(&mut self, scale: f32) {
        self.dpi_scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        if self.dpi_scale != 1.0 {
            unsafe {
                gp::GdipScaleWorldTransform(
                    self.g,
                    self.dpi_scale,
                    self.dpi_scale,
                    MATRIX_ORDER_PREPEND,
                )
            };
        }
    }

    /// 让圆角描边的外沿保持原半径，中心路径半径随向内收缩量同步减小。
    fn aligned_stroke(
        &self,
        rect: Rect,
        radius: Corners,
        line_width: f32,
    ) -> Option<(Rect, Corners, f32)> {
        let (path, aligned_width) = pixel_aligned_stroke(rect, line_width, self.dpi_scale)?;
        let physical_width = aligned_width * self.dpi_scale;
        let align_radius = |value: f32| {
            ((value.max(0.0) * self.dpi_scale).round() - physical_width / 2.0).max(0.0)
                / self.dpi_scale
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
}

/// Color → GDI+ ARGB（0xAARRGGBB）。
fn argb(c: Color) -> u32 {
    let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (f(c.a) << 24) | (f(c.r) << 16) | (f(c.g) << 8) | f(c.b)
}

fn ri(v: f32) -> i32 {
    v.round() as i32
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ImageKey {
    Path(String),
    Bytes(usize),
    Svg(usize, u32, u32),
}

struct CachedImage {
    image: *mut gp::GpImage,
    stream: *mut c_void,
    _bytes: Option<Arc<Vec<u8>>>,
    _pixels: Option<Vec<u8>>,
}

impl CachedImage {
    unsafe fn from_path(path: &str) -> Option<Self> {
        let mut image: *mut gp::GpImage = std::ptr::null_mut();
        let path = wide(path);
        if gp::GdipLoadImageFromFile(path.as_ptr(), &mut image) != 0 || image.is_null() {
            return None;
        }
        Some(Self {
            image,
            stream: std::ptr::null_mut(),
            _bytes: None,
            _pixels: None,
        })
    }

    unsafe fn from_bytes(bytes: Arc<Vec<u8>>) -> Option<Self> {
        let len = u32::try_from(bytes.len()).ok()?;
        let stream = SHCreateMemStream(bytes.as_ptr(), len);
        if stream.is_null() {
            return None;
        }
        let mut image: *mut gp::GpImage = std::ptr::null_mut();
        if gp::GdipLoadImageFromStream(stream, &mut image) != 0 || image.is_null() {
            release_stream(stream);
            return None;
        }
        Some(Self {
            image,
            stream,
            _bytes: Some(bytes),
            _pixels: None,
        })
    }

    unsafe fn from_rgba(rgba: &[u8], width: u32, height: u32) -> Option<Self> {
        let mut pixels = rgba.to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let mut bitmap: *mut gp::GpBitmap = std::ptr::null_mut();
        if gp::GdipCreateBitmapFromScan0(
            width as i32,
            height as i32,
            (width * 4) as i32,
            crate::gdiplus::PIXEL_FORMAT_32BPP_PARGB,
            pixels.as_ptr(),
            &mut bitmap,
        ) != 0
            || bitmap.is_null()
        {
            return None;
        }
        Some(Self {
            image: bitmap as *mut gp::GpImage,
            stream: std::ptr::null_mut(),
            _bytes: None,
            _pixels: Some(pixels),
        })
    }
}

impl Drop for CachedImage {
    fn drop(&mut self) {
        unsafe {
            if !self.image.is_null() {
                gp::GdipDisposeImage(self.image);
            }
            if !self.stream.is_null() {
                release_stream(self.stream);
            }
        }
    }
}

unsafe fn release_stream(stream: *mut c_void) {
    let vtable = *(stream as *mut *mut windows_sys::core::IUnknown_Vtbl);
    ((*vtable).Release)(stream);
}

/// 每窗口图片缓存；随窗口状态释放，保证原生图片先于 GDI+ shutdown 销毁。
#[derive(Default)]
pub struct ImageCache {
    images: HashMap<ImageKey, CachedImage>,
}

impl ImageCache {
    fn path(&mut self, path: &str) -> Option<*mut gp::GpImage> {
        let key = ImageKey::Path(path.to_string());
        if !self.images.contains_key(&key) {
            let image = unsafe { CachedImage::from_path(path) }?;
            self.images.insert(key.clone(), image);
        }
        self.images.get(&key).map(|entry| entry.image)
    }

    fn bytes(&mut self, bytes: &Arc<Vec<u8>>) -> Option<*mut gp::GpImage> {
        let key = ImageKey::Bytes(Arc::as_ptr(bytes) as usize);
        if !self.images.contains_key(&key) {
            let image = unsafe { CachedImage::from_bytes(Arc::clone(bytes)) }?;
            self.images.insert(key.clone(), image);
        }
        self.images.get(&key).map(|entry| entry.image)
    }

    fn svg(&mut self, bytes: &Arc<Vec<u8>>, width: u32, height: u32) -> Option<*mut gp::GpImage> {
        let key = ImageKey::Svg(Arc::as_ptr(bytes) as usize, width, height);
        if !self.images.contains_key(&key) {
            let rgba = flexui_svg::rasterize(bytes, width, height)?;
            let mut image = unsafe { CachedImage::from_rgba(&rgba, width, height) }?;
            image._bytes = Some(Arc::clone(bytes));
            self.images.insert(key.clone(), image);
        }
        self.images.get(&key).map(|entry| entry.image)
    }
}

/// tint 颜色矩阵（行主 5x5）：输出 RGB=目标色、A=原 alpha（黑图/任意图 → 目标色形状）。
fn tint_matrix(c: Color) -> gp::ColorMatrix {
    let mut m = [0.0f32; 25];
    m[3 * 5 + 3] = 1.0; // A = inA
    m[4 * 5] = c.r.clamp(0.0, 1.0);
    m[4 * 5 + 1] = c.g.clamp(0.0, 1.0);
    m[4 * 5 + 2] = c.b.clamp(0.0, 1.0);
    m[4 * 5 + 4] = 1.0;
    gp::ColorMatrix { m }
}

/// UTF-8 → 以 NUL 结尾的 UTF-16。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl GdiCanvas<'_> {
    /// 生成四角独立圆角的矩形路径（各角半径 <=0 时该角为直角）。
    /// 使用浮点路径接口，精度更好，且能触发 GDI+ 的抗锯齿路径。
    unsafe fn build_round_path(&self, r: Rect, radius: Corners) -> *mut gp::GpPath {
        let mut path: *mut gp::GpPath = std::ptr::null_mut();
        gp::GdipCreatePath(FILLMODE_ALTERNATE, &mut path);
        let x = r.left();
        let y = r.top();
        let w = r.size.width;
        let h = r.size.height;
        let hw = w / 2.0;
        let hh = h / 2.0;
        let clamp = |v: f32| v.max(0.0).min(hw).min(hh);
        let tl = clamp(radius.tl);
        let tr = clamp(radius.tr);
        let br = clamp(radius.br);
        let bl = clamp(radius.bl);

        if tl <= 0.0 && tr <= 0.0 && br <= 0.0 && bl <= 0.0 {
            gp::GdipAddPathRectangle(path, x, y, w, h);
            return path;
        }
        // 每角一段 90° 弧（半径 0 时 AddPathArc 的 0 尺寸椭圆退化为该角点，即直角）+ 自动连线。
        gp::GdipAddPathArc(path, x, y, tl * 2.0, tl * 2.0, 180.0, 90.0);
        gp::GdipAddPathArc(path, x + w - tr * 2.0, y, tr * 2.0, tr * 2.0, 270.0, 90.0);
        gp::GdipAddPathArc(
            path,
            x + w - br * 2.0,
            y + h - br * 2.0,
            br * 2.0,
            br * 2.0,
            0.0,
            90.0,
        );
        gp::GdipAddPathArc(path, x, y + h - bl * 2.0, bl * 2.0, bl * 2.0, 90.0, 90.0);
        gp::GdipClosePathFigure(path);
        path
    }

    /// 绘制一个已加载的 GpImage（tint 用 ColorMatrix 重着色；按 fit 布局）。
    unsafe fn draw_gpimage(
        &self,
        img: *mut gp::GpImage,
        rect: Rect,
        tint: Option<Color>,
        fit: &ImageFit,
        density: f32,
    ) {
        let mut iw = 0u32;
        let mut ih = 0u32;
        gp::GdipGetImageWidth(img, &mut iw);
        gp::GdipGetImageHeight(img, &mut ih);
        if iw == 0 || ih == 0 || density <= 0.0 {
            return;
        }
        let (iw, ih) = (iw as f32, ih as f32);
        let (logical_iw, logical_ih) = (iw / density, ih / density);
        // tint：颜色矩阵把 RGB 置为目标色、保留 alpha（黑图/任意图 → 目标色形状）。
        let mut attr: *mut gp::GpImageAttributes = std::ptr::null_mut();
        if let Some(c) = tint {
            gp::GdipCreateImageAttributes(&mut attr);
            let m = tint_matrix(c);
            gp::GdipSetImageAttributesColorMatrix(
                attr,
                gp::ColorAdjustTypeDefault,
                1,
                &m,
                std::ptr::null(),
                gp::ColorMatrixFlagsDefault,
            );
        }
        let (dx, dy, dw, dh) = (rect.left(), rect.top(), rect.size.width, rect.size.height);
        match fit {
            ImageFit::Stretch => self.draw_piece(img, dx, dy, dw, dh, 0.0, 0.0, iw, ih, attr),
            ImageFit::Center => {
                let x = dx + (dw - logical_iw) / 2.0;
                let y = dy + (dh - logical_ih) / 2.0;
                self.draw_piece(img, x, y, logical_iw, logical_ih, 0.0, 0.0, iw, ih, attr);
            }
            ImageFit::Tile => {
                let mut y = dy;
                while y < dy + dh {
                    let mut x = dx;
                    while x < dx + dw {
                        let tw = (dx + dw - x).min(logical_iw);
                        let th = (dy + dh - y).min(logical_ih);
                        self.draw_piece(
                            img,
                            x,
                            y,
                            tw,
                            th,
                            0.0,
                            0.0,
                            tw * density,
                            th * density,
                            attr,
                        );
                        x += logical_iw;
                    }
                    y += logical_ih;
                }
            }
            ImageFit::NinePatch(ins) => {
                let (sl, sr, st, sb) = (
                    ins.left * density,
                    ins.right * density,
                    ins.top * density,
                    ins.bottom * density,
                );
                let cs = [0.0, sl, iw - sr, iw];
                let cd = [dx, dx + ins.left, dx + dw - ins.right, dx + dw];
                let rs = [0.0, st, ih - sb, ih];
                let rd = [dy, dy + ins.top, dy + dh - ins.bottom, dy + dh];
                for r in 0..3 {
                    for c in 0..3 {
                        let (sx, sw) = (cs[c], cs[c + 1] - cs[c]);
                        let (sy, sh) = (rs[r], rs[r + 1] - rs[r]);
                        let (ox, ow) = (cd[c], cd[c + 1] - cd[c]);
                        let (oy, oh) = (rd[r], rd[r + 1] - rd[r]);
                        if sw > 0.0 && sh > 0.0 && ow > 0.0 && oh > 0.0 {
                            self.draw_piece(img, ox, oy, ow, oh, sx, sy, sw, sh, attr);
                        }
                    }
                }
            }
        }
        if !attr.is_null() {
            gp::GdipDisposeImageAttributes(attr);
        }
    }

    /// 绘制源矩形→目标矩形（带可选颜色属性）。
    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_piece(
        &self,
        img: *mut gp::GpImage,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        attr: *mut gp::GpImageAttributes,
    ) {
        gp::GdipDrawImageRectRect(
            self.g,
            img,
            dx,
            dy,
            dw,
            dh,
            sx,
            sy,
            sw,
            sh,
            UNIT_PIXEL,
            attr,
            0,
            std::ptr::null_mut(),
        );
    }

    fn draw_path_source(
        &mut self,
        path: &str,
        density: f32,
        rect: Rect,
        tint: Option<Color>,
        fit: &ImageFit,
    ) {
        if let Some(cache) = self.image_cache.as_deref_mut() {
            if let Some(image) = cache.path(path) {
                unsafe { self.draw_gpimage(image, rect, tint, fit, density) };
            }
            return;
        }
        if let Some(image) = unsafe { CachedImage::from_path(path) } {
            unsafe { self.draw_gpimage(image.image, rect, tint, fit, density) };
        }
    }

    fn draw_bytes_source(
        &mut self,
        bytes: &Arc<Vec<u8>>,
        density: f32,
        rect: Rect,
        tint: Option<Color>,
        fit: &ImageFit,
    ) {
        if let Some(cache) = self.image_cache.as_deref_mut() {
            if let Some(image) = cache.bytes(bytes) {
                unsafe { self.draw_gpimage(image, rect, tint, fit, density) };
            }
            return;
        }
        if let Some(image) = unsafe { CachedImage::from_bytes(Arc::clone(bytes)) } {
            unsafe { self.draw_gpimage(image.image, rect, tint, fit, density) };
        }
    }

    fn draw_svg_source(
        &mut self,
        bytes: &Arc<Vec<u8>>,
        rect: Rect,
        tint: Option<Color>,
        fit: &ImageFit,
    ) {
        let logical = match fit {
            ImageFit::Stretch => (rect.size.width, rect.size.height),
            _ => flexui_svg::intrinsic_size(bytes).unwrap_or((rect.size.width, rect.size.height)),
        };
        let width = ((logical.0 * self.dpi_scale).round() as u32).max(1);
        let height = ((logical.1 * self.dpi_scale).round() as u32).max(1);
        if let Some(cache) = self.image_cache.as_deref_mut() {
            if let Some(image) = cache.svg(bytes, width, height) {
                unsafe { self.draw_gpimage(image, rect, tint, fit, self.dpi_scale) };
            }
            return;
        }
        let Some(rgba) = flexui_svg::rasterize(bytes, width, height) else {
            return;
        };
        if let Some(image) = unsafe { CachedImage::from_rgba(&rgba, width, height) } {
            unsafe { self.draw_gpimage(image.image, rect, tint, fit, self.dpi_scale) };
        }
    }

    /// 用给定字族名/系统字体创建字体；返回 (font, family)，调用方负责释放。
    unsafe fn make_font(&self, font: &Font) -> (*mut gp::GpFont, *mut gp::GpFontFamily) {
        let mut family: *mut gp::GpFontFamily = std::ptr::null_mut();
        let family_name = font.family.as_deref().unwrap_or(DEFAULT_FONT_FAMILY);
        let wname = wide(family_name);
        gp::GdipCreateFontFamilyFromName(wname.as_ptr(), std::ptr::null_mut(), &mut family);
        if family.is_null() {
            // 字体缺失时回退到系统通用 sans-serif（wine/windows 均可用）。
            gp::GdipGetGenericFontFamilySansSerif(&mut family);
        }
        // GDI+ FontStyle 位标志：Bold=1 Italic=2 Underline=4。
        let style =
            (font.bold as i32) | ((font.italic as i32) << 1) | ((font.underline as i32) << 2);
        let mut f: *mut gp::GpFont = std::ptr::null_mut();
        if !family.is_null() {
            gp::GdipCreateFont(family, font.size, style, UNIT_PIXEL, &mut f);
        }
        (f, family)
    }
}

impl Canvas for GdiCanvas<'_> {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        unsafe {
            let mut brush: *mut gp::GpSolidFill = std::ptr::null_mut();
            gp::GdipCreateSolidFill(argb(color), &mut brush);
            gp::GdipFillRectangleI(
                self.g,
                brush as *mut gp::GpBrush,
                ri(rect.left()),
                ri(rect.top()),
                ri(rect.size.width),
                ri(rect.size.height),
            );
            gp::GdipDeleteBrush(brush as *mut gp::GpBrush);
        }
    }

    fn stroke_rect(&mut self, rect: Rect, color: Color, line_width: f32) {
        let Some((path, _, line_width)) = self.aligned_stroke(rect, Corners::default(), line_width)
        else {
            return;
        };
        unsafe {
            let mut pen: *mut gp::GpPen = std::ptr::null_mut();
            gp::GdipCreatePen1(argb(color), line_width, UNIT_PIXEL, &mut pen);
            gp::GdipDrawRectangle(
                self.g,
                pen,
                path.left(),
                path.top(),
                path.size.width,
                path.size.height,
            );
            gp::GdipDeletePen(pen);
        }
    }

    fn fill_round_rect(&mut self, rect: Rect, radius: Corners, color: Color) {
        unsafe {
            let path = self.build_round_path(rect, radius);
            let mut brush: *mut gp::GpSolidFill = std::ptr::null_mut();
            gp::GdipCreateSolidFill(argb(color), &mut brush);
            gp::GdipFillPath(self.g, brush as *mut gp::GpBrush, path);
            gp::GdipDeleteBrush(brush as *mut gp::GpBrush);
            gp::GdipDeletePath(path);
        }
    }

    fn fill_gradient_rect(
        &mut self,
        rect: Rect,
        radius: Corners,
        from: Color,
        to: Color,
        vertical: bool,
    ) {
        unsafe {
            let path = self.build_round_path(rect, radius);
            let (p1, p2) = if vertical {
                (
                    gp::PointF {
                        X: rect.left(),
                        Y: rect.top(),
                    },
                    gp::PointF {
                        X: rect.left(),
                        Y: rect.bottom(),
                    },
                )
            } else {
                (
                    gp::PointF {
                        X: rect.left(),
                        Y: rect.top(),
                    },
                    gp::PointF {
                        X: rect.right(),
                        Y: rect.top(),
                    },
                )
            };
            let mut brush: *mut gp::GpLineGradient = std::ptr::null_mut();
            let st = gp::GdipCreateLineBrush(&p1, &p2, argb(from), argb(to), 0, &mut brush);
            if st == 0 && !brush.is_null() {
                gp::GdipFillPath(self.g, brush as *mut gp::GpBrush, path);
                gp::GdipDeleteBrush(brush as *mut gp::GpBrush);
            } else {
                // 回退纯色。
                let mut solid: *mut gp::GpSolidFill = std::ptr::null_mut();
                gp::GdipCreateSolidFill(argb(from), &mut solid);
                gp::GdipFillPath(self.g, solid as *mut gp::GpBrush, path);
                gp::GdipDeleteBrush(solid as *mut gp::GpBrush);
            }
            gp::GdipDeletePath(path);
        }
    }

    fn stroke_round_rect(&mut self, rect: Rect, radius: Corners, color: Color, line_width: f32) {
        let Some((path_rect, path_radius, line_width)) =
            self.aligned_stroke(rect, radius, line_width)
        else {
            return;
        };
        unsafe {
            let path = self.build_round_path(path_rect, path_radius);
            let mut pen: *mut gp::GpPen = std::ptr::null_mut();
            gp::GdipCreatePen1(argb(color), line_width, UNIT_PIXEL, &mut pen);
            gp::GdipDrawPath(self.g, pen, path);
            gp::GdipDeletePen(pen);
            gp::GdipDeletePath(path);
        }
    }

    fn draw_text(&mut self, text: &str, origin: Point, font: &Font, color: Color) {
        if text.is_empty() {
            return;
        }
        unsafe {
            let (f, family) = self.make_font(font);
            if f.is_null() {
                if !family.is_null() {
                    gp::GdipDeleteFontFamily(family);
                }
                return;
            }
            let mut brush: *mut gp::GpSolidFill = std::ptr::null_mut();
            gp::GdipCreateSolidFill(argb(color), &mut brush);
            let layout = gp::RectF {
                X: origin.x,
                Y: origin.y,
                Width: 10000.0,
                Height: 10000.0,
            };
            let wtext = wide(text);
            gp::GdipDrawString(
                self.g,
                wtext.as_ptr(),
                -1,
                f,
                &layout,
                std::ptr::null(),
                brush as *const gp::GpBrush,
            );
            gp::GdipDeleteBrush(brush as *mut gp::GpBrush);
            gp::GdipDeleteFont(f);
            gp::GdipDeleteFontFamily(family);
        }
    }

    fn draw_text_advance(&mut self, text: &str, origin: Point, font: &Font, color: Color) {
        if text.is_empty() {
            return;
        }
        unsafe {
            let (f, family) = self.make_font(font);
            if f.is_null() {
                if !family.is_null() {
                    gp::GdipDeleteFontFamily(family);
                }
                return;
            }
            let mut brush: *mut gp::GpSolidFill = std::ptr::null_mut();
            gp::GdipCreateSolidFill(argb(color), &mut brush);
            let layout = gp::RectF {
                X: origin.x,
                Y: origin.y,
                Width: 10000.0,
                Height: 10000.0,
            };
            let mut format: *mut gp::GpStringFormat = std::ptr::null_mut();
            gp::GdipStringFormatGetGenericTypographic(&mut format);
            if !format.is_null() {
                let mut flags = 0;
                gp::GdipGetStringFormatFlags(format, &mut flags);
                gp::GdipSetStringFormatFlags(
                    format,
                    flags
                        | gp::StringFormatFlagsMeasureTrailingSpaces
                        | gp::StringFormatFlagsNoWrap,
                );
            }
            let wtext = wide(text);
            gp::GdipDrawString(
                self.g,
                wtext.as_ptr(),
                -1,
                f,
                &layout,
                format,
                brush as *const gp::GpBrush,
            );
            if !format.is_null() {
                gp::GdipDeleteStringFormat(format);
            }
            gp::GdipDeleteBrush(brush as *mut gp::GpBrush);
            gp::GdipDeleteFont(f);
            gp::GdipDeleteFontFamily(family);
        }
    }

    fn measure_text(&self, text: &str, font: &Font) -> Size {
        unsafe {
            let (f, family) = self.make_font(font);
            if f.is_null() {
                if !family.is_null() {
                    gp::GdipDeleteFontFamily(family);
                }
                return Size::new(0.0, font.size * 1.2);
            }
            let layout = gp::RectF {
                X: 0.0,
                Y: 0.0,
                Width: 10000.0,
                Height: 10000.0,
            };
            let mut bbox = gp::RectF {
                X: 0.0,
                Y: 0.0,
                Width: 0.0,
                Height: 0.0,
            };
            let wtext = wide(text);
            gp::GdipMeasureString(
                self.g,
                wtext.as_ptr(),
                -1,
                f,
                &layout,
                std::ptr::null(),
                &mut bbox,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            gp::GdipDeleteFont(f);
            gp::GdipDeleteFontFamily(family);
            Size::new(bbox.Width, bbox.Height)
        }
    }

    fn measure_text_advance_size(&self, text: &str, font: &Font) -> Size {
        if text.is_empty() {
            return Size::default();
        }
        unsafe {
            let (f, family) = self.make_font(font);
            if f.is_null() {
                if !family.is_null() {
                    gp::GdipDeleteFontFamily(family);
                }
                return Size::new(0.0, font.size * 1.2);
            }
            let layout = gp::RectF {
                X: 0.0,
                Y: 0.0,
                Width: 10000.0,
                Height: 10000.0,
            };
            let mut bbox = gp::RectF {
                X: 0.0,
                Y: 0.0,
                Width: 0.0,
                Height: 0.0,
            };
            let mut format: *mut gp::GpStringFormat = std::ptr::null_mut();
            gp::GdipStringFormatGetGenericTypographic(&mut format);
            if !format.is_null() {
                let mut flags = 0;
                gp::GdipGetStringFormatFlags(format, &mut flags);
                gp::GdipSetStringFormatFlags(
                    format,
                    flags
                        | gp::StringFormatFlagsMeasureTrailingSpaces
                        | gp::StringFormatFlagsNoWrap,
                );
            }
            let wtext = wide(text);
            gp::GdipMeasureString(
                self.g,
                wtext.as_ptr(),
                -1,
                f,
                &layout,
                format,
                &mut bbox,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if !format.is_null() {
                gp::GdipDeleteStringFormat(format);
            }
            gp::GdipDeleteFont(f);
            gp::GdipDeleteFontFamily(family);
            Size::new(bbox.Width, bbox.Height)
        }
    }

    fn layout_text(&self, text: &str, font: &Font) -> TextLayout {
        crate::text::layout_text(text, font).unwrap_or_else(|| {
            let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
            for index in 0..=text.chars().count() {
                let prefix = text.chars().take(index).collect::<String>();
                boundaries.push(flexui_gfx::TextBoundary {
                    char_index: index,
                    x: self.measure_text_advance(&prefix, font),
                });
            }
            let size = self.measure_text_advance_size(text, font);
            TextLayout::new(
                text,
                font.clone(),
                size,
                font.size,
                (size.height - font.size).max(0.0),
                boundaries,
            )
        })
    }

    fn draw_text_layout(&mut self, layout: &TextLayout, origin: Point, color: Color) {
        if unsafe {
            crate::text::draw_text_layout(self.g, self.dpi_scale, self.clip, layout, origin, color)
        } {
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
            return;
        }
        self.draw_text_advance(layout.text(), origin, layout.font(), color);
    }

    fn draw_image(&mut self, source: &ImageSource, rect: Rect, tint: Option<Color>, fit: ImageFit) {
        match source {
            ImageSource::Path(path) => self.draw_path_source(path, 1.0, rect, tint, &fit),
            ImageSource::Bytes(bytes) => self.draw_bytes_source(bytes, 1.0, rect, tint, &fit),
            ImageSource::ScaledPath(path, density) => {
                self.draw_path_source(path, *density, rect, tint, &fit)
            }
            ImageSource::ScaledBytes(bytes, density) => {
                self.draw_bytes_source(bytes, *density, rect, tint, &fit)
            }
            ImageSource::Svg(bytes) => self.draw_svg_source(bytes, rect, tint, &fit),
        }
    }

    fn save(&mut self) {
        let mut state: u32 = 0;
        unsafe { gp::GdipSaveGraphics(self.g, &mut state) };
        self.saved.push(state);
        self.saved_clips.push(self.clip);
    }

    fn restore(&mut self) {
        if let Some(state) = self.saved.pop() {
            unsafe { gp::GdipRestoreGraphics(self.g, state) };
            self.clip = self.saved_clips.pop().flatten();
        }
    }

    fn clip_rect(&mut self, rect: Rect) {
        self.clip = Some(match self.clip {
            Some(current) => intersect_rect(current, rect),
            None => rect,
        });
        unsafe {
            gp::GdipSetClipRect(
                self.g,
                rect.left(),
                rect.top(),
                rect.size.width,
                rect.size.height,
                COMBINE_INTERSECT,
            );
        }
    }

    fn clip_round_rect(&mut self, rect: Rect, radius: Corners) {
        self.clip = Some(match self.clip {
            Some(current) => intersect_rect(current, rect),
            None => rect,
        });
        unsafe {
            let path = self.build_round_path(rect, radius);
            gp::GdipSetClipPath(self.g, path, COMBINE_INTERSECT);
            gp::GdipDeletePath(path);
        }
    }
}

fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let left = a.left().max(b.left());
    let top = a.top().max(b.top());
    let right = a.right().min(b.right()).max(left);
    let bottom = a.bottom().min(b.bottom()).max(top);
    Rect::new(left, top, right - left, bottom - top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 嵌套裁剪取矩形交集() {
        assert_eq!(
            intersect_rect(
                Rect::new(10.0, 10.0, 40.0, 30.0),
                Rect::new(20.0, 5.0, 40.0, 20.0),
            ),
            Rect::new(20.0, 10.0, 30.0, 15.0)
        );
        assert_eq!(
            intersect_rect(
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Rect::new(20.0, 20.0, 5.0, 5.0),
            ),
            Rect::new(20.0, 20.0, 0.0, 0.0)
        );
    }
}
