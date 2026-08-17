use flexui::{MainProxy, ResourceManager, Skin, WindowCtx, WindowImpl};

use crate::resources::resources;

pub(crate) const CLOSED: &str = "app:close-prompt:closed";
pub(crate) const HIDE_ONCE: &str = "app:close-prompt:hide";
pub(crate) const HIDE_ALWAYS: &str = "app:close-prompt:hide-always";
pub(crate) const EXIT_ONCE: &str = "app:close-prompt:exit";
pub(crate) const EXIT_ALWAYS: &str = "app:close-prompt:exit-always";

pub(crate) struct ClosePrompt {
    owner: MainProxy,
}

impl ClosePrompt {
    pub(crate) fn new(owner: MainProxy) -> Self {
        Self { owner }
    }
}

impl WindowImpl for ClosePrompt {
    fn skin(&self) -> Skin {
        Skin::res("close_prompt.xml")
    }

    fn resources(&self) -> ResourceManager {
        resources()
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "close_prompt" => ctx.close(),
            "confirm_close" => {
                let exit = ctx.is_selected("close_exit").unwrap_or(false);
                let remember = ctx.is_selected("close_remember").unwrap_or(false);
                let message = match (exit, remember) {
                    (false, false) => HIDE_ONCE,
                    (false, true) => HIDE_ALWAYS,
                    (true, false) => EXIT_ONCE,
                    (true, true) => EXIT_ALWAYS,
                };
                self.owner.send(message);
                ctx.close();
            }
            _ => {}
        }
    }

    fn on_closed(&mut self) {
        self.owner.send(CLOSED);
    }
}
