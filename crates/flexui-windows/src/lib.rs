//! flexui-windows：Windows 平台后端（L1）。
//!
//! 用 Win32 管理窗口、GDI+ 自绘控件。仅在 Windows 目标编译；非 Windows 平台为空 crate。
//! 与 macOS 后端对位，向 flexui-core 提供同一套 `Canvas` 与 `run`。

#![cfg(windows)]
// GDI+ 句柄本质是裸指针，接口按惯例暴露安全签名（内部经空值检查后使用）。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod canvas;
mod gdiplus;
mod window;

pub use canvas::GdiCanvas;
pub use gdiplus::{Gdiplus, OffscreenBitmap};
pub use window::{run, WindowConfig};

use flexui_core::{layout_node, paint_tree, Rect, Widget};

/// 离屏渲染整棵控件树到 w×h 的位图，返回多个采样点的 ARGB 像素值。
///
/// 用于无窗口环境（测试/截图）验证 GDI+ 绘制管线与抗锯齿。
pub fn render_tree_samples(
    root: &mut dyn Widget,
    width: i32,
    height: i32,
    points: &[(i32, i32)],
) -> Option<Vec<u32>> {
    let _gp = Gdiplus::startup()?;
    let off = OffscreenBitmap::new(width, height)?;
    let mut cv = GdiCanvas::new(off.graphics());
    // 不透明底，保证读回像素判定稳定（与窗口双缓冲一致）。
    cv.clear(flexui_core::Color::WHITE);
    layout_node(root, Rect::new(0.0, 0.0, width as f32, height as f32), &cv);
    paint_tree(&*root, &mut cv);
    drop(cv);
    Some(points.iter().map(|&(x, y)| off.get_pixel(x, y)).collect())
}

/// 便捷：只采样一个点。
pub fn render_tree_argb(root: &mut dyn Widget, width: i32, height: i32, sample: (i32, i32)) -> Option<u32> {
    render_tree_samples(root, width, height, &[sample]).map(|v| v[0])
}
