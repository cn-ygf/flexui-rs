//! flexui-linux：Linux 平台后端（系统图形接口 Cairo + Pango）。
//!
//! 分层：`canvas` 用 Cairo/Pango 实现平台无关的 `flexui_gfx::Canvas`（渲染到内存
//! ImageSurface）；窗口/事件（后续）用 X11 直接把像素 blit 到窗口。

mod canvas;

pub use canvas::CairoCanvas;
