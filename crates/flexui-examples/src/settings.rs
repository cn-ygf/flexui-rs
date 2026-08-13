use flexui::{LocalizedStringResource, ResourceManager, Skin, WindowCtx, WindowImpl};

use crate::resources::resources;

pub(crate) struct SettingsDialog;

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
                let selected = ctx
                    .with("language", |widget| widget.selected_index())
                    .flatten();
                match selected {
                    Some(0) => {
                        let _ = ctx.set_system_locale();
                    }
                    Some(1) => {
                        let _ = ctx.set_locale("zh-Hans");
                    }
                    Some(2) => {
                        let _ = ctx.set_locale("zh-Hant");
                    }
                    Some(3) => {
                        let _ = ctx.set_locale("en");
                    }
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
