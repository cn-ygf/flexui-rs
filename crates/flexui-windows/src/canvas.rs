//! Windows 画布：用 GDI+ flat API 实现平台无关的 `flexui_gfx::Canvas`。
//!
//! 与 macOS 的 CgCanvas 对位。坐标为逻辑像素、左上原点，与统一坐标系一致。

use flexui_geometry::{Color, Corners, Point, Rect, Size};
use flexui_gfx::{Canvas, Font, ImageFit, ImageSource};
use windows_sys::Win32::Graphics::GdiPlus as gp;

use crate::gdiplus::{
    COMBINE_INTERSECT, FILLMODE_ALTERNATE, MATRIX_ORDER_PREPEND, SMOOTHING_ANTIALIAS,
    TEXT_HINT_CLEARTYPE, UNIT_PIXEL,
};

/// GDI+ 画布，持有一个 Graphics 指针（来自窗口 HDC 或离屏位图）。
pub struct GdiCanvas {
    g: *mut gp::GpGraphics,
    saved: Vec<u32>,
}

impl GdiCanvas {
    /// 用给定 Graphics 构造，并开启抗锯齿与清晰文字。
    pub fn new(g: *mut gp::GpGraphics) -> Self {
        unsafe {
            gp::GdipSetSmoothingMode(g, SMOOTHING_ANTIALIAS);
            gp::GdipSetTextRenderingHint(g, TEXT_HINT_CLEARTYPE);
        }
        Self { g, saved: Vec::new() }
    }

    /// 用不透明底色清屏（保证 ClearType 文字有实底、blit 无残留）。
    pub fn clear(&mut self, color: Color) {
        unsafe { gp::GdipGraphicsClear(self.g, argb(color)) };
    }

    /// 施加 DPI 缩放：之后所有逻辑坐标按 scale 放大到物理像素（HiDPI 清晰）。
    pub fn set_dpi_scale(&mut self, scale: f32) {
        if scale != 1.0 {
            unsafe { gp::GdipScaleWorldTransform(self.g, scale, scale, MATRIX_ORDER_PREPEND) };
        }
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

/// 内嵌图片落临时文件的唯一名计数。
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

impl GdiCanvas {
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
        gp::GdipAddPathArc(path, x + w - br * 2.0, y + h - br * 2.0, br * 2.0, br * 2.0, 0.0, 90.0);
        gp::GdipAddPathArc(path, x, y + h - bl * 2.0, bl * 2.0, bl * 2.0, 90.0, 90.0);
        gp::GdipClosePathFigure(path);
        path
    }

    /// 从文件路径加载图片，绘制到 rect（支持 tint 换色 + fit 渲染方式）。
    unsafe fn draw_image_file(&self, path: &str, rect: Rect, tint: Option<Color>, fit: &ImageFit) {
        let wpath = wide(path);
        let mut img: *mut gp::GpImage = std::ptr::null_mut();
        if gp::GdipLoadImageFromFile(wpath.as_ptr(), &mut img) == 0 && !img.is_null() {
            self.draw_gpimage(img, rect, tint, fit);
            gp::GdipDisposeImage(img);
        }
    }

    /// 从 RGBA(premultiplied) 字节建 GDI+ 位图并绘制（供 SVG 光栅化结果用）。
    unsafe fn draw_rgba(&self, rgba: &[u8], w: u32, h: u32, rect: Rect, tint: Option<Color>, fit: &ImageFit) {
        // tiny_skia RGBA(premult) → GDI+ 期望 BGRA 字节序（小端 PARGB）。
        let mut bgra = rgba.to_vec();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let mut img: *mut gp::GpBitmap = std::ptr::null_mut();
        let stride = (w * 4) as i32;
        if gp::GdipCreateBitmapFromScan0(
            w as i32,
            h as i32,
            stride,
            crate::gdiplus::PIXEL_FORMAT_32BPP_PARGB,
            bgra.as_ptr(),
            &mut img,
        ) == 0
            && !img.is_null()
        {
            self.draw_gpimage(img as *mut gp::GpImage, rect, tint, fit);
            gp::GdipDisposeImage(img as *mut gp::GpImage);
        }
    }

    /// 绘制一个已加载的 GpImage（tint 用 ColorMatrix 重着色；按 fit 布局）。
    unsafe fn draw_gpimage(&self, img: *mut gp::GpImage, rect: Rect, tint: Option<Color>, fit: &ImageFit) {
        let mut iw = 0u32;
        let mut ih = 0u32;
        gp::GdipGetImageWidth(img, &mut iw);
        gp::GdipGetImageHeight(img, &mut ih);
        let (iw, ih) = (iw as i32, ih as i32);
        if iw == 0 || ih == 0 {
            return;
        }
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
        let (dx, dy, dw, dh) = (
            ri(rect.left()),
            ri(rect.top()),
            ri(rect.size.width),
            ri(rect.size.height),
        );
        match fit {
            ImageFit::Stretch => self.draw_piece(img, dx, dy, dw, dh, 0, 0, iw, ih, attr),
            ImageFit::Center => {
                let x = dx + (dw - iw) / 2;
                let y = dy + (dh - ih) / 2;
                self.draw_piece(img, x, y, iw, ih, 0, 0, iw, ih, attr);
            }
            ImageFit::Tile => {
                let mut y = dy;
                while y < dy + dh {
                    let mut x = dx;
                    while x < dx + dw {
                        let tw = (dx + dw - x).min(iw);
                        let th = (dy + dh - y).min(ih);
                        self.draw_piece(img, x, y, tw, th, 0, 0, tw, th, attr);
                        x += iw;
                    }
                    y += ih;
                }
            }
            ImageFit::NinePatch(ins) => {
                let (sl, sr, st, sb) = (ins.left as i32, ins.right as i32, ins.top as i32, ins.bottom as i32);
                let cs = [0, sl, iw - sr, iw];
                let cd = [dx, dx + sl, dx + dw - sr, dx + dw];
                let rs = [0, st, ih - sb, ih];
                let rd = [dy, dy + st, dy + dh - sb, dy + dh];
                for r in 0..3 {
                    for c in 0..3 {
                        let (sx, sw) = (cs[c], cs[c + 1] - cs[c]);
                        let (sy, sh) = (rs[r], rs[r + 1] - rs[r]);
                        let (ox, ow) = (cd[c], cd[c + 1] - cd[c]);
                        let (oy, oh) = (rd[r], rd[r + 1] - rd[r]);
                        if sw > 0 && sh > 0 && ow > 0 && oh > 0 {
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
        dx: i32,
        dy: i32,
        dw: i32,
        dh: i32,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        attr: *mut gp::GpImageAttributes,
    ) {
        gp::GdipDrawImageRectRectI(
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

    /// 用给定字族名/系统字体创建字体；返回 (font, family)，调用方负责释放。
    unsafe fn make_font(&self, font: &Font) -> (*mut gp::GpFont, *mut gp::GpFontFamily) {
        let mut family: *mut gp::GpFontFamily = std::ptr::null_mut();
        if let Some(name) = &font.family {
            let wname = wide(name);
            gp::GdipCreateFontFamilyFromName(wname.as_ptr(), std::ptr::null_mut(), &mut family);
        }
        if family.is_null() {
            // 回退到系统通用 sans-serif（wine/windows 均可用）。
            gp::GdipGetGenericFontFamilySansSerif(&mut family);
        }
        // GDI+ FontStyle 位标志：Bold=1 Italic=2 Underline=4。
        let style = (font.bold as i32) | ((font.italic as i32) << 1) | ((font.underline as i32) << 2);
        let mut f: *mut gp::GpFont = std::ptr::null_mut();
        if !family.is_null() {
            gp::GdipCreateFont(family, font.size, style, UNIT_PIXEL, &mut f);
        }
        (f, family)
    }
}

impl Canvas for GdiCanvas {
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
        unsafe {
            let mut pen: *mut gp::GpPen = std::ptr::null_mut();
            gp::GdipCreatePen1(argb(color), line_width, UNIT_PIXEL, &mut pen);
            gp::GdipDrawRectangleI(
                self.g,
                pen,
                ri(rect.left()),
                ri(rect.top()),
                ri(rect.size.width),
                ri(rect.size.height),
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
                    gp::PointF { X: rect.left(), Y: rect.top() },
                    gp::PointF { X: rect.left(), Y: rect.bottom() },
                )
            } else {
                (
                    gp::PointF { X: rect.left(), Y: rect.top() },
                    gp::PointF { X: rect.right(), Y: rect.top() },
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
        unsafe {
            let path = self.build_round_path(rect, radius);
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

    fn draw_image(&mut self, source: &ImageSource, rect: Rect, tint: Option<Color>, fit: ImageFit) {
        match source {
            ImageSource::Path(p) => unsafe { self.draw_image_file(p, rect, tint, &fit) },
            ImageSource::Bytes(b) => {
                // GDI+ 从内存需 IStream，成本高；这里落临时文件再加载（简单可靠）。
                // 注：每次绘制解码，后续可加 LRU 解码缓存优化。
                use std::io::Write;
                let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut tmp = std::env::temp_dir();
                tmp.push(format!("flexui_img_{}_{}.dat", std::process::id(), n));
                if std::fs::File::create(&tmp)
                    .and_then(|mut f| f.write_all(b))
                    .is_ok()
                {
                    if let Some(s) = tmp.to_str() {
                        unsafe { self.draw_image_file(s, rect, tint, &fit) };
                    }
                    let _ = std::fs::remove_file(&tmp);
                }
            }
            ImageSource::Svg(b) => {
                // 按目标尺寸 2× 超采样光栅化（配合窗口 DPI world transform，高分屏清晰）。
                let pw = ((rect.size.width * 2.0).round() as u32).max(1);
                let ph = ((rect.size.height * 2.0).round() as u32).max(1);
                if let Some(rgba) = flexui_svg::rasterize(b, pw, ph) {
                    unsafe { self.draw_rgba(&rgba, pw, ph, rect, tint, &fit) };
                }
            }
        }
    }

    fn save(&mut self) {
        let mut state: u32 = 0;
        unsafe { gp::GdipSaveGraphics(self.g, &mut state) };
        self.saved.push(state);
    }

    fn restore(&mut self) {
        if let Some(state) = self.saved.pop() {
            unsafe { gp::GdipRestoreGraphics(self.g, state) };
        }
    }

    fn clip_rect(&mut self, rect: Rect) {
        unsafe {
            gp::GdipSetClipRectI(
                self.g,
                ri(rect.left()),
                ri(rect.top()),
                ri(rect.size.width),
                ri(rect.size.height),
                COMBINE_INTERSECT,
            );
        }
    }
}
