#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use flexui::{
    build_window, Color, DirProvider, ImageFit, ImageSource, Insets, MenuStyle, Rect,
    LocalizedStringResource, Localizer, ResourceManager, ScrollBarStyle, Skin, Window, WindowCtx,
    WindowImpl,
};
use std::collections::HashMap;
use std::sync::OnceLock;

const ORIGINAL_LOGIN_XML: &str = include_str!("../assets/data/original_login.xml");
const ORIGINAL_CHS_XML: &str = include_str!("../assets/data/chs.xml");
const ORIGINAL_CHT_XML: &str = include_str!("../assets/data/cht_tw.xml");
const ORIGINAL_EN_XML: &str = include_str!("../assets/data/en.xml");

#[derive(Clone, Copy)]
enum NationLanguage {
    SimplifiedChinese,
    TraditionalChinese,
    English,
}

fn nation_language(locale: Option<&str>) -> NationLanguage {
    let locale = locale.unwrap_or("zh-Hans").to_ascii_lowercase();
    if locale == "en" || locale.starts_with("en-") {
        NationLanguage::English
    } else if locale.contains("hant")
        || locale.starts_with("zh-tw")
        || locale.starts_with("zh-hk")
        || locale.starts_with("zh-mo")
    {
        NationLanguage::TraditionalChinese
    } else {
        NationLanguage::SimplifiedChinese
    }
}

fn escape_bare_ampersands(xml: &str) -> String {
    let mut escaped = String::with_capacity(xml.len());
    for (index, character) in xml.char_indices() {
        if character == '&' {
            let tail = &xml[index..];
            if !["&amp;", "&quot;", "&apos;", "&lt;", "&gt;", "&#"]
                .iter()
                .any(|entity| tail.starts_with(entity))
            {
                escaped.push_str("&amp;");
                continue;
            }
        }
        escaped.push(character);
    }
    escaped
}

fn parse_nation_codes(language_xml: &str) -> Vec<(String, String)> {
    let language_xml = escape_bare_ampersands(language_xml);
    let language = roxmltree::Document::parse(&language_xml)
        .expect("原版语言文件必须是有效 XML");
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
                Some((labels.get(id)?.to_string(), format!("nation_entry_{id}_{code}")))
            } else if node.has_tag_name("Label") {
                let title = node.attribute("text")?;
                Some((title.to_string(), format!("nation_header_{title}")))
            } else {
                None
            }
        })
        .collect()
}

fn nation_codes(locale: Option<&str>) -> &'static [(String, String)] {
    static SIMPLIFIED: OnceLock<Vec<(String, String)>> = OnceLock::new();
    static TRADITIONAL: OnceLock<Vec<(String, String)>> = OnceLock::new();
    static ENGLISH: OnceLock<Vec<(String, String)>> = OnceLock::new();
    match nation_language(locale) {
        NationLanguage::SimplifiedChinese => {
            SIMPLIFIED.get_or_init(|| parse_nation_codes(ORIGINAL_CHS_XML))
        }
        NationLanguage::TraditionalChinese => {
            TRADITIONAL.get_or_init(|| parse_nation_codes(ORIGINAL_CHT_XML))
        }
        NationLanguage::English => ENGLISH.get_or_init(|| parse_nation_codes(ORIGINAL_EN_XML)),
    }
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
            "btn_start" => ctx.set_localized_text("home_status", "app.status.steam_selected"),
            _ => {}
        }
    }
}

struct LoginDialog {
    nation_code: String,
    nation_item: String,
}

impl Default for LoginDialog {
    fn default() -> Self {
        Self { nation_code: "86".into(), nation_item: "nation_entry_1063_86".into() }
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
                    nation_codes(ctx.locale().as_deref()).to_vec(),
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
                    Some(self.nation_item.clone()),
                );
            }
            _ if name.starts_with("nation_entry_") => {
                self.nation_item = name.to_owned();
                self.nation_code = name.rsplit('_').next().unwrap_or("86").to_owned();
                ctx.set_text("nation_code_text", format!("+{}", self.nation_code));
            }
            "get_code" => ctx.set_localized_text("login_status", "login.status.no_sms"),
            "btn_login" => ctx.set_localized_text("login_status", "login.status.no_submit"),
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
            "language" => {
                let selected = ctx.with("language", |widget| widget.selected_index()).flatten();
                match selected {
                    Some(0) => { let _ = ctx.set_system_locale(); }
                    Some(1) => { let _ = ctx.set_locale("zh-Hans"); }
                    Some(2) => { let _ = ctx.set_locale("zh-Hant"); }
                    Some(3) => { let _ = ctx.set_locale("en"); }
                    _ => {}
                }
                ctx.set_localized_text("settings_status", "settings.locale_changed");
            }
            "set_startup" | "set_desktop" | "set_auto" | "set_sleep" | "set_audio"
            | "set_notice" => {
                let enabled = ctx.is_selected(name).unwrap_or(false);
                ctx.set_localized_text(
                    "settings_status",
                    LocalizedStringResource::new(if enabled {
                        "settings.enabled"
                    } else {
                        "settings.disabled"
                    }),
                );
            }
            _ => {}
        }
    }
}

fn main() {
    let localizer = Localizer::new("zh-Hans").expect("开发语言标签必须有效");
    let resource_manager = resources();
    localizer.load_json_res(&resource_manager, "i18n/zh-Hans.json")
        .expect("加载简体中文本地化资源失败");
    localizer.load_json_res(&resource_manager, "i18n/en.json")
        .expect("加载英文本地化资源失败");
    localizer.load_json_res(&resource_manager, "i18n/zh-Hant.json")
        .expect("加载繁体中文本地化资源失败");
    flexui::set_application_localizer(localizer);

    #[cfg(target_os = "macos")]
    flexui::set_application_icon(include_bytes!("../assets/app.icns"));
    Window::new(UuExample).center().run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn nation_code_catalogs_match_the_original_layout() {
        let simplified = nation_codes(Some("zh-Hans"));
        let traditional = nation_codes(Some("zh-TW"));
        let english = nation_codes(Some("en-US"));
        assert_eq!(simplified.len(), traditional.len());
        assert_eq!(simplified.len(), english.len());
        assert_eq!(simplified[0].0, "中国(+86)");
        assert_eq!(traditional[0].0, "中國(+86)");
        assert_eq!(english[0].0, "China (+86)");
        assert_eq!(
            simplified.iter().map(|(_, name)| name).collect::<HashSet<_>>().len(),
            simplified.len()
        );
    }
}
