#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod close_prompt;
mod login;
mod resources;
mod settings;
mod tray;

use flexui::Window;

use crate::app::MainWindow;

fn main() {
    resources::install_localizer();

    #[cfg(target_os = "macos")]
    flexui::set_application_icon(include_bytes!("../assets/app.icns"));
    // Linux 用 .ico（image crate 不识别 .icns）→ 设 _NET_WM_ICON 任务栏图标。
    #[cfg(target_os = "linux")]
    flexui::set_application_icon(include_bytes!("../assets/app.ico"));
    Window::new(MainWindow::default()).center().run();
}
