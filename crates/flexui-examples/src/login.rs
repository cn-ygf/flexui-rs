use flexui::{
    Color, ImageFit, Insets, MenuStyle, Rect, ResourceManager, ScrollBarStyle, Size, Skin,
    WindowCtx, WindowImpl,
};

use crate::resources::{original_bitmap, resources};

pub(crate) struct LoginDialog {
    nation_code: String,
    nation_item: String,
}

impl Default for LoginDialog {
    fn default() -> Self {
        Self {
            nation_code: "86".into(),
            nation_item: "nation_entry_86".into(),
        }
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
                    nation_codes(ctx.locale().as_deref()),
                    nation_menu_style(),
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

fn nation_menu_style() -> MenuStyle {
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
        background_fit: Some(ImageFit::NinePatch(Insets::new(28.0, 24.0, 28.0, 32.0))),
        selected_image: Some(original_bitmap(include_bytes!(
            "../assets/label/ic_selected@2.00x.png"
        ))),
        selected_image_size: Size::new(12.0, 9.0),
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
        ..Default::default()
    }
}

#[derive(Clone, Copy)]
enum NationLanguage {
    SimplifiedChinese,
    TraditionalChinese,
    English,
}

struct Nation {
    code: &'static str,
    simplified: &'static str,
    traditional: &'static str,
    english: &'static str,
}

const NATIONS: &[Nation] = &[
    Nation {
        code: "86",
        simplified: "中国(+86)",
        traditional: "中國(+86)",
        english: "China (+86)",
    },
    Nation {
        code: "852",
        simplified: "中国香港(+852)",
        traditional: "中國香港(+852)",
        english: "Hong Kong, China (+852)",
    },
    Nation {
        code: "853",
        simplified: "中国澳门(+853)",
        traditional: "中國澳門(+853)",
        english: "Macao, China (+853)",
    },
    Nation {
        code: "886",
        simplified: "中国台湾(+886)",
        traditional: "中國台灣(+886)",
        english: "Taiwan, China (+886)",
    },
    Nation {
        code: "81",
        simplified: "日本(+81)",
        traditional: "日本(+81)",
        english: "Japan (+81)",
    },
    Nation {
        code: "82",
        simplified: "韩国(+82)",
        traditional: "韓國(+82)",
        english: "South Korea (+82)",
    },
    Nation {
        code: "1",
        simplified: "美国(+1)",
        traditional: "美國(+1)",
        english: "United States (+1)",
    },
    Nation {
        code: "61",
        simplified: "澳大利亚(+61)",
        traditional: "澳大利亞(+61)",
        english: "Australia (+61)",
    },
];

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

fn nation_codes(locale: Option<&str>) -> Vec<(String, String)> {
    let language = nation_language(locale);
    NATIONS
        .iter()
        .map(|nation| {
            let text = match language {
                NationLanguage::SimplifiedChinese => nation.simplified,
                NationLanguage::TraditionalChinese => nation.traditional,
                NationLanguage::English => nation.english,
            };
            (text.to_owned(), format!("nation_entry_{}", nation.code))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn nation_code_catalogs_are_localized_and_unique() {
        let simplified = nation_codes(Some("zh-Hans"));
        let traditional = nation_codes(Some("zh-TW"));
        let english = nation_codes(Some("en-US"));
        assert_eq!(simplified.len(), traditional.len());
        assert_eq!(simplified.len(), english.len());
        assert_eq!(simplified.len(), NATIONS.len());
        assert_eq!(simplified[0].0, "中国(+86)");
        assert_eq!(traditional[0].0, "中國(+86)");
        assert_eq!(english[0].0, "China (+86)");
        assert_eq!(
            simplified
                .iter()
                .map(|(_, name)| name)
                .collect::<HashSet<_>>()
                .len(),
            simplified.len()
        );
    }
}
