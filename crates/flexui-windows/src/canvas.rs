//! Windows 画布：用 GDI+ flat API 实现平台无关的 `flexui_gfx::Canvas`。
//!
//! 与 macOS 的 CgCanvas 对位。坐标为逻辑像素、左上原点，与统一坐标系一致。

use flexui_geometry::{Color, Corners, Point, Rect, Size};
use flexui_gfx::{Canvas, Font, ImageSource};
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
        let mut f: *mut gp::GpFont = std::ptr::null_mut();
        if !family.is_null() {
            gp::GdipCreateFont(family, font.size, 0, UNIT_PIXEL, &mut f);
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

    fn draw_image(&mut self, source: &ImageSource, rect: Rect) {
        let ImageSource::Path(p) = source;
        unsafe {
            let wpath = wide(p);
            let mut img: *mut gp::GpImage = std::ptr::null_mut();
            // 加载失败（文件不存在/格式不支持）时静默跳过，不影响其它绘制。
            if gp::GdipLoadImageFromFile(wpath.as_ptr(), &mut img) == 0 && !img.is_null() {
                gp::GdipDrawImageRectI(
                    self.g,
                    img,
                    ri(rect.left()),
                    ri(rect.top()),
                    ri(rect.size.width),
                    ri(rect.size.height),
                );
                gp::GdipDisposeImage(img);
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
