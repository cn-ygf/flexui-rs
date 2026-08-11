use flexui::{
    DirProvider, ResourceManager, Skin, TitlebarMode, Window, WindowConfig, WindowCtx, WindowImpl,
};

struct UuExample;

impl WindowImpl for UuExample {
    fn config(&self) -> WindowConfig {
        WindowConfig::new("UU 加速器 · flexui XML 示例", 1000.0, 688.0)
            .resizable(false)
            .titlebar(TitlebarMode::None)
    }

    fn skin(&self) -> Skin {
        Skin::res("app.xml")
    }

    fn resources(&self) -> ResourceManager {
        let assets = format!("{}/assets", env!("CARGO_MANIFEST_DIR"));
        let mut resources = ResourceManager::new();
        resources.mount(DirProvider::new(assets));
        resources
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "open_login" => {
                ctx.set_visible("settings_overlay", false);
                ctx.set_visible("login_overlay", true);
            }
            "login_overlay" | "close_login" => ctx.set_visible("login_overlay", false),
            "open_settings" => {
                ctx.set_visible("login_overlay", false);
                ctx.set_visible("settings_overlay", true);
            }
            "settings_overlay" | "close_settings" => {
                ctx.set_visible("settings_overlay", false)
            }
            "minimize" => ctx.minimize(),
            "close_window" => ctx.close(),
            "btn_start" => ctx.set_text("home_status", "已选择 Steam，准备开始加速（演示）"),
            "btn_login" => ctx.set_text("login_status", "登录按钮已触发；这是本地 XML 示例，不会发送账号信息"),
            "btn_save" => ctx.set_text("settings_status", "设置已保存到当前演示会话"),
            "btn_reset" => ctx.set_text("settings_status", "设置已恢复为示例默认值"),
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
    Window::new(UuExample).center().run();
}
