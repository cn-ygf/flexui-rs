#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use flexui::{
    build_window, Color, DirProvider, Insets, MenuStyle, Rect, ResourceManager, Skin, Window,
    WindowCtx, WindowImpl,
};

fn resources() -> ResourceManager {
    let assets = format!("{}/assets", env!("CARGO_MANIFEST_DIR"));
    let mut resources = ResourceManager::new();
    resources.mount(DirProvider::new(assets));
    resources
}

struct UuExample;

impl WindowImpl for UuExample {
    fn skin(&self) -> Skin {
        Skin::res("app.xml")
    }

    fn resources(&self) -> ResourceManager {
        resources()
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "open_login" => {
                if let Ok(dialog) = build_window(LoginDialog::default()) {
                    ctx.open_modal(dialog);
                }
            }
            "open_settings" => {
                if let Ok(dialog) = build_window(SettingsDialog) {
                    ctx.open_modal(dialog);
                }
            }
            "minimize" => ctx.minimize(),
            "close_window" => ctx.close(),
            "btn_start" => ctx.set_text("home_status", "已选择 Steam，准备开始加速（演示）"),
            _ => {}
        }
    }
}

struct LoginDialog {
    nation_code: &'static str,
}

impl Default for LoginDialog {
    fn default() -> Self {
        Self { nation_code: "86" }
    }
}

impl WindowImpl for LoginDialog {
    fn skin(&self) -> Skin {
        Skin::res("login.xml")
    }

    fn resources(&self) -> ResourceManager {
        resources()
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "close_login" => ctx.close(),
            "nation_code" | "nation_code_text" | "nation_code_arrow" => {
                let anchor = ctx
                    .with("nation_code", |widget| widget.base().rect)
                    .unwrap_or(Rect::new(326.0, 128.0, 68.0, 44.0));
                ctx.open_styled_menu(
                    anchor,
                    vec![
                        ("中国大陆  +86".into(), "nation_86".into()),
                        ("中国香港  +852".into(), "nation_852".into()),
                        ("中国澳门  +853".into(), "nation_853".into()),
                        ("中国台湾  +886".into(), "nation_886".into()),
                        ("韩国  +82".into(), "nation_82".into()),
                        ("日本  +81".into(), "nation_81".into()),
                        ("美国/加拿大  +1".into(), "nation_1".into()),
                        ("澳大利亚  +61".into(), "nation_61".into()),
                    ],
                    MenuStyle {
                        background: Color::from_u8(57, 63, 116, 255),
                        border: Color::from_u8(57, 63, 116, 255),
                        text: Color::from_u8(188, 192, 212, 255),
                        hot_text: Color::from_u8(188, 192, 212, 255),
                        selected_text: Color::WHITE,
                        hot_background: Color::from_u8(71, 75, 133, 255),
                        row_height: 32.0,
                        width: Some(294.0),
                        item_padding: Insets::new(32.0, 0.0, 12.0, 0.0),
                        panel_padding: 4.0,
                        corner_radius: 6.0,
                    },
                    Some(format!("nation_{}", self.nation_code)),
                );
            }
            "nation_86" | "nation_852" | "nation_853" | "nation_886" | "nation_82"
            | "nation_81" | "nation_1" | "nation_61" => {
                let code = name.trim_start_matches("nation_");
                self.nation_code = match code {
                    "86" => "86",
                    "852" => "852",
                    "853" => "853",
                    "886" => "886",
                    "82" => "82",
                    "81" => "81",
                    "1" => "1",
                    "61" => "61",
                    _ => self.nation_code,
                };
                ctx.set_text("nation_code_text", format!("+{}", self.nation_code));
            }
            "get_code" => ctx.set_text("login_status", "演示界面不会发送短信"),
            "btn_login" => ctx.set_text("login_status", "演示界面不会提交账号信息"),
            _ => {}
        }
    }
}

struct SettingsDialog;

impl WindowImpl for SettingsDialog {
    fn skin(&self) -> Skin {
        Skin::res("settings.xml")
    }

    fn resources(&self) -> ResourceManager {
        resources()
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "close_settings" => ctx.close(),
            "set_startup" | "set_desktop" | "set_auto" | "set_sleep" | "set_audio"
            | "set_notice" => {
                let enabled = ctx.is_selected(name).unwrap_or(false);
                ctx.set_text(
                    "settings_status",
                    if enabled { "设置项已开启" } else { "设置项已关闭" },
                );
            }
            _ => {}
        }
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    flexui::set_application_icon(include_bytes!("../assets/app.icns"));
    Window::new(UuExample).center().run();
}
