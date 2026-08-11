//! GDI+ 基础设施（L1，Windows）：进程级初始化 RAII + 离屏位图助手。
//!
//! 使用 windows-sys 的 GDI+ flat API（`Gdip*` 函数，句柄 + 错误码风格）。

use windows_sys::Win32::Graphics::GdiPlus as gp;

/// PixelFormat32bppARGB（windows-sys 未导出该常量，用其固定数值）。
pub const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;
/// PixelFormat32bppPARGB（预乘 alpha，供 SVG 光栅化结果上传）。
pub const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;

// 常用 GDI+ 枚举值（windows-sys 里是 i32 常量，这里集中别名）。
pub const UNIT_PIXEL: gp::Unit = 2;
pub const SMOOTHING_ANTIALIAS: gp::SmoothingMode = 4;
pub const INTERPOLATION_HIGH_QUALITY_BICUBIC: gp::InterpolationMode = 7;
pub const PIXEL_OFFSET_HIGH_QUALITY: gp::PixelOffsetMode = 2;
pub const TEXT_HINT_CLEARTYPE: gp::TextRenderingHint = 5;
pub const FILLMODE_ALTERNATE: gp::FillMode = 0;
pub const COMBINE_INTERSECT: gp::CombineMode = 1;
pub const MATRIX_ORDER_PREPEND: gp::MatrixOrder = 0;

/// GDI+ 进程级初始化，Drop 时自动 Shutdown。
pub struct Gdiplus {
    token: usize,
}

impl Gdiplus {
    /// 初始化 GDI+。失败返回 None。
    pub fn startup() -> Option<Self> {
        let input = gp::GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token: usize = 0;
        // 输出结构体不使用，置零即可。
        let mut output: gp::GdiplusStartupOutput = unsafe { std::mem::zeroed() };
        let status = unsafe { gp::GdiplusStartup(&mut token, &input, &mut output) };
        if status == 0 {
            Some(Self { token })
        } else {
            None
        }
    }
}

impl Drop for Gdiplus {
    fn drop(&mut self) {
        unsafe { gp::GdiplusShutdown(self.token) };
    }
}

/// 离屏 ARGB 位图 + 其绘图上下文，用于无窗口渲染（测试/截图）。
pub struct OffscreenBitmap {
    bmp: *mut gp::GpBitmap,
    graphics: *mut gp::GpGraphics,
}

impl OffscreenBitmap {
    /// 创建 w×h 的 32bpp ARGB 离屏位图。
    pub fn new(width: i32, height: i32) -> Option<Self> {
        let mut bmp: *mut gp::GpBitmap = std::ptr::null_mut();
        let s = unsafe {
            gp::GdipCreateBitmapFromScan0(
                width,
                height,
                0,
                PIXEL_FORMAT_32BPP_ARGB,
                std::ptr::null(),
                &mut bmp,
            )
        };
        if s != 0 || bmp.is_null() {
            return None;
        }
        let mut graphics: *mut gp::GpGraphics = std::ptr::null_mut();
        let s = unsafe { gp::GdipGetImageGraphicsContext(bmp as *mut gp::GpImage, &mut graphics) };
        if s != 0 || graphics.is_null() {
            unsafe { gp::GdipDisposeImage(bmp as *mut gp::GpImage) };
            return None;
        }
        Some(Self { bmp, graphics })
    }

    /// 绘图上下文指针（交给 GdiCanvas 使用）。
    pub fn graphics(&self) -> *mut gp::GpGraphics {
        self.graphics
    }

    /// 位图作为 GpImage 的指针（用于整块 blit 到窗口）。
    pub fn image(&self) -> *mut gp::GpImage {
        self.bmp as *mut gp::GpImage
    }

    /// 读回某像素的 ARGB 值。
    pub fn get_pixel(&self, x: i32, y: i32) -> u32 {
        let mut color: u32 = 0;
        unsafe { gp::GdipBitmapGetPixel(self.bmp, x, y, &mut color) };
        color
    }
}

impl Drop for OffscreenBitmap {
    fn drop(&mut self) {
        unsafe {
            gp::GdipDeleteGraphics(self.graphics);
            gp::GdipDisposeImage(self.bmp as *mut gp::GpImage);
        }
    }
}
