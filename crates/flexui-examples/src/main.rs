#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use flexui::{
    build_window, Color, DirProvider, ImageFit, ImageSource, Insets, MenuStyle, Rect,
    ResourceManager, ScrollBarStyle, Skin, Window, WindowCtx, WindowImpl,
};
use std::collections::HashMap;
use std::sync::OnceLock;

const ORIGINAL_LOGIN_XML: &str = include_str!("../assets/data/original_login.xml");
const ORIGINAL_CHS_XML: &str = include_str!("../assets/data/chs.xml");

fn nation_codes() -> &'static [(String, String)] {
    static CODES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    CODES.get_or_init(|| {
        let language = roxmltree::Document::parse(ORIGINAL_CHS_XML)
            .expect("原版简体中文语言文件必须是有效 XML");
        let labels: HashMap<&str, &str> = language
            .descendants()
            .filter(|node| node.has_tag_name("rlang"))
            .filter_map(|node| Some((node.attribute("id")?, node.attribute("text")?)))
            .collect();
        let layout = roxmltree::Document::parse(ORIGINAL_LOGIN_XML)
            .expect("原版登录布局必须是有效 XML");
        let list = layout
            .descendants()
            .find(|node| node.attribute("name") == Some("nation_flags_list"))
            .expect("原版登录布局必须包含 nation_flags_list");
        list.children()
            .filter(|node| node.is_element())
            .filter_map(|node| {
                if node.attribute("userdata") == Some("national_flag") {
                    let code = node.attribute("name")?;
                    let resource = node.attribute("text")?;
                    let id = resource.strip_prefix("%{")?.strip_suffix('}')?;
                    Some((labels.get(id)?.to_string(), format!("nation_{code}")))
                } else if node.has_tag_name("Label") {
                    let title = node.attribute("text")?;
                    Some((title.to_string(), format!("nation_header_{title}")))
                } else {
                    None
                }
            })
            .collect()
    })
}

fn original_bitmap(bytes: &'static [u8]) -> ImageSource {
    ImageSource::bytes_scaled(bytes.to_vec(), 2.0)
}

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
    nation_code: String,
}

impl Default for LoginDialog {
    fn default() -> Self {
        Self { nation_code: "86".into() }
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
                    nation_codes().to_vec(),
                    MenuStyle {
                        background: Color::from_u8(57, 63, 116, 255),
                        border: Color::from_u8(0, 0, 0, 0),
                        text: Color::from_u8(188, 192, 212, 255),
                        hot_text: Color::from_u8(188, 192, 212, 255),
                        selected_text: Color::from_u8(80, 200, 190, 255),
                        hot_background: Color::from_u8(71, 75, 133, 255),
                        row_height: 32.0,
                        width: Some(294.0),
                        height: Some(228.0),
                        item_padding: Insets::new(44.0, 0.0, 8.0, 0.0),
                        panel_padding: Insets::new(24.0, 16.0, 20.0, 24.0),
                        corner_radius: 0.0,
                        background_image: Some(original_bitmap(include_bytes!(
                            "../assets/common/dropdwon_bg@2.00x.png"
                        ))),
                        background_fit: Some(ImageFit::NinePatch(Insets::new(
                            28.0, 24.0, 28.0, 32.0,
                        ))),
                        selected_image: Some(original_bitmap(include_bytes!(
                            "../assets/label/ic_selected@2.00x.png"
                        ))),
                        selected_image_size: flexui::Size::new(12.0, 9.0),
                        header_name_prefix: Some("nation_header_".into()),
                        header_text: Color::from_u8(87, 94, 169, 255),
                        header_height: 20.0,
                        header_padding: Insets::new(16.0, 0.0, 0.0, 0.0),
                        scrollbar: ScrollBarStyle {
                            width: 6.0,
                            min_thumb_height: 16.0,
                            thumb_image: Some(original_bitmap(include_bytes!(
                                "../assets/scrollbar/scorll-bar-normal@2.00x.png"
                            ))),
                            thumb_fit: ImageFit::NinePatch(Insets::all(2.0)),
                            ..Default::default()
                        },
                        window_margin: Insets::new(0.0, 0.0, 14.0, 28.0),
                    },
                    Some(format!("nation_{}", self.nation_code)),
                );
            }
            _ if name.starts_with("nation_") => {
                self.nation_code = name.trim_start_matches("nation_").to_string();
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
