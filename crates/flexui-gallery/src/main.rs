#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod http_demo;
mod resources;
mod themes;

use app::GalleryWindow;
use flexui::Window;

fn main() {
    if std::env::args().any(|arg| arg == "--dark") {
        flexui::set_application_theme(flexui::Theme::dark());
    }
    Window::new(GalleryWindow::default()).center().run();
}
