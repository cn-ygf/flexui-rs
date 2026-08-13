use flexui::{ControlEvent, ResourceManager, Skin, Theme, ThemeMode, WindowCtx, WindowImpl};

use crate::{resources, themes};

pub(crate) struct GalleryWindow;

impl WindowImpl for GalleryWindow {
    fn skin(&self) -> Skin {
        Skin::res("gallery.xml")
    }

    fn resources(&self) -> ResourceManager {
        resources::resources()
    }

    fn on_init(&mut self, ctx: &mut WindowCtx) {
        if ctx
            .theme()
            .is_some_and(|theme| theme.mode == ThemeMode::Dark)
        {
            ctx.set_selected("theme_switch", true);
        }
    }

    fn on_control_event(&mut self, name: &str, event: &ControlEvent, ctx: &mut WindowCtx) {
        if let ("theme_switch", ControlEvent::SelectedChanged(dark)) = (name, event) {
            ctx.set_theme(if *dark { Theme::dark() } else { Theme::light() });
        }
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "apply_bilibili_theme" => {
                ctx.set_selected("theme_switch", false);
                ctx.set_theme(themes::bilibili_theme());
            }
            "restore_default_theme" => {
                ctx.set_selected("theme_switch", false);
                ctx.set_theme(Theme::light());
            }
            _ => {}
        }
    }
}
