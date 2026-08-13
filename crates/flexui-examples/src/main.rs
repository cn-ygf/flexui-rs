#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod login;
mod resources;
mod settings;

use flexui::Window;

use crate::app::MainWindow;

fn main() {
    resources::install_localizer();

    #[cfg(target_os = "macos")]
    flexui::set_application_icon(include_bytes!("../assets/app.icns"));
    Window::new(MainWindow).center().run();
}
