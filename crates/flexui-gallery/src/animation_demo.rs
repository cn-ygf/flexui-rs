//! 控件动画示例的交互逻辑。

use flexui::{AnimProp, Easing, WindowCtx};

const EASING_TARGETS: [(&str, Easing); 4] = [
    ("animation_linear", Easing::Linear),
    ("animation_ease_in", Easing::EaseIn),
    ("animation_ease_out", Easing::EaseOut),
    ("animation_ease_in_out", Easing::EaseInOut),
];

pub(crate) struct AnimationDemo {
    easing_at_end: bool,
    transition_visible: bool,
    progress_high: bool,
}

impl Default for AnimationDemo {
    fn default() -> Self {
        Self {
            easing_at_end: false,
            transition_visible: true,
            progress_high: false,
        }
    }
}

impl AnimationDemo {
    pub(crate) fn handle_click(&mut self, name: &str, ctx: &mut WindowCtx) -> bool {
        match name {
            "animation_compare" => {
                self.easing_at_end = !self.easing_at_end;
                self.animate_easing_targets(ctx, if self.easing_at_end { 300.0 } else { 0.0 });
            }
            "animation_reset_easing" => {
                self.easing_at_end = false;
                for (target, _) in EASING_TARGETS {
                    ctx.animate(target, AnimProp::TranslateX, 0.0, 0.35, Easing::EaseOut);
                }
            }
            "animation_show_transition" => self.set_transition_visible(true, ctx),
            "animation_hide_transition" => self.set_transition_visible(false, ctx),
            "animation_toggle_transition" => {
                self.set_transition_visible(!self.transition_visible, ctx)
            }
            "animation_progress_toggle" => {
                self.progress_high = !self.progress_high;
                ctx.animate(
                    "animation_progress",
                    AnimProp::Value,
                    if self.progress_high { 0.9 } else { 0.15 },
                    0.65,
                    Easing::EaseInOut,
                );
            }
            _ => return false,
        }
        true
    }

    fn animate_easing_targets(&self, ctx: &mut WindowCtx, destination: f32) {
        for (target, easing) in EASING_TARGETS {
            ctx.animate(target, AnimProp::TranslateX, destination, 0.85, easing);
        }
    }

    fn set_transition_visible(&mut self, visible: bool, ctx: &mut WindowCtx) {
        self.transition_visible = visible;
        ctx.set_visible_animated("animation_transition_panel", visible);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui::{load_window_res, Context, WindowHandle};

    struct TestWindow;

    impl WindowHandle for TestWindow {
        fn set_title(&mut self, _title: &str) {}
        fn close(&mut self) {}
        fn minimize(&mut self) {}
        fn maximize(&mut self) {}
        fn restore(&mut self) {}
    }

    #[test]
    fn animation_buttons_create_tween_and_transition_requests() {
        let mut xml_context = Context::new();
        xml_context.register_widget_factory("StatusBadge", |element, _| {
            Ok(Box::new(flexui::Label::new(
                element.attr("text-verbatim").unwrap_or("Status"),
            )))
        });
        let mut doc =
            load_window_res(&crate::resources::resources(), "gallery.xml", &xml_context).unwrap();
        let mut window = TestWindow;
        let mut ctx = WindowCtx::new(doc.root.as_mut(), &mut window);
        let mut demo = AnimationDemo::default();

        assert!(demo.handle_click("animation_compare", &mut ctx));
        let requests = ctx.take_anim_requests();
        assert_eq!(requests.len(), 4);
        assert!(requests.iter().all(|request| {
            request.prop == AnimProp::TranslateX && request.to == 300.0 && request.dur_secs == 0.85
        }));

        assert!(demo.handle_click("animation_hide_transition", &mut ctx));
        let requests = ctx.take_anim_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].name, "animation_transition_panel");
        assert_eq!(requests[0].prop, AnimProp::TranslateY);

        assert!(demo.handle_click("animation_progress_toggle", &mut ctx));
        let requests = ctx.take_anim_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].name, "animation_progress");
        assert_eq!(requests[0].prop, AnimProp::Value);
        assert_eq!(requests[0].easing, Easing::EaseInOut);

        assert!(!demo.handle_click("unknown", &mut ctx));
    }
}
