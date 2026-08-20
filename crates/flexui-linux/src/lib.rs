//! flexui-linux：Linux 平台后端（系统图形接口 Cairo + Pango，窗口用 X11/x11rb）。
//!
//! 分层：`canvas` 用 Cairo/Pango 实现平台无关的 `flexui_gfx::Canvas`（渲染到内存
//! ImageSurface）；`window` 用 X11 建窗口、跑事件循环，把像素 blit 到窗口。
//!
//! 整个 crate 仅在 Linux 目标下编译；mac/win 上为空壳（不引入 cairo/pango）。
#![cfg(target_os = "linux")]

mod canvas;
mod clipboard;
mod dialog;
mod menu;
mod window;

pub use canvas::CairoCanvas;
pub use clipboard::{get_text as clipboard_get_text, set_text as clipboard_set_text};
pub use dialog::show_dialog;
pub use window::{run_multi, set_application_icon};
