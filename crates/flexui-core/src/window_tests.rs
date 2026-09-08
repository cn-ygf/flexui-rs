use super::*;
use crate::{Dispatcher, Easing, Panel, Transition, TransitionEdge};

struct TestWindow;

impl WindowHandle for TestWindow {
    fn set_title(&mut self, _title: &str) {}
    fn close(&mut self) {}
    fn minimize(&mut self) {}
    fn maximize(&mut self) {}
    fn restore(&mut self) {}
}

fn apply_requests(dispatcher: &mut Dispatcher, root: &mut dyn Widget, requests: Vec<AnimRequest>) {
    for request in requests {
        assert!(dispatcher.animate(
            root,
            &request.name,
            request.prop,
            request.to,
            request.dur_secs,
            request.easing,
        ));
    }
}

#[test]
fn 显隐过渡_滑入后保持可见_滑出后才隐藏() {
    let transition = Transition::slide(TransitionEdge::Bottom, 100.0)
        .duration(1.0)
        .easing(Easing::Linear);
    let mut root = Panel::new()
        .name("panel")
        .visible(false)
        .transition(transition);
    let mut window = TestWindow;
    let mut dispatcher = Dispatcher::new();

    let open_requests = {
        let mut ctx = WindowCtx::new(&mut root, &mut window);
        ctx.set_visible_animated("panel", true);
        ctx.take_anim_requests()
    };
    assert!(root.base().visible);
    assert_eq!(root.base().transform.translation.y, 100.0);
    apply_requests(&mut dispatcher, &mut root, open_requests);
    dispatcher.tick_anims(&mut root, 0.5);
    assert_eq!(root.base().transform.translation.y, 50.0);
    dispatcher.tick_anims(&mut root, 0.5);
    assert!(root.base().visible);
    assert_eq!(root.base().transform.translation.y, 0.0);

    let close_requests = {
        let mut ctx = WindowCtx::new(&mut root, &mut window);
        ctx.set_visible_animated("panel", false);
        ctx.take_anim_requests()
    };
    assert!(root.base().visible, "退出动画播放期间仍应绘制和参与命中");
    apply_requests(&mut dispatcher, &mut root, close_requests);
    dispatcher.tick_anims(&mut root, 0.5);
    assert!(root.base().visible);
    assert_eq!(root.base().transform.translation.y, 50.0);
    dispatcher.tick_anims(&mut root, 0.5);
    assert!(!root.base().visible);
    assert_eq!(root.base().transform.translation.y, 0.0);
    assert!(dispatcher.take_layout(), "隐藏完成后应重新布局");
}

#[test]
fn 显隐过渡_退出途中重新打开会从当前位置反向播放() {
    let transition = Transition::slide(TransitionEdge::Bottom, 100.0)
        .duration(1.0)
        .easing(Easing::Linear);
    let mut root = Panel::new().name("panel").transition(transition);
    let mut window = TestWindow;
    let mut dispatcher = Dispatcher::new();

    let close_requests = {
        let mut ctx = WindowCtx::new(&mut root, &mut window);
        ctx.set_visible_animated("panel", false);
        ctx.take_anim_requests()
    };
    apply_requests(&mut dispatcher, &mut root, close_requests);
    dispatcher.tick_anims(&mut root, 0.5);
    assert_eq!(root.base().transform.translation.y, 50.0);

    let reopen_requests = {
        let mut ctx = WindowCtx::new(&mut root, &mut window);
        ctx.set_visible_animated("panel", true);
        ctx.take_anim_requests()
    };
    apply_requests(&mut dispatcher, &mut root, reopen_requests);
    dispatcher.tick_anims(&mut root, 1.0);
    assert!(root.base().visible);
    assert_eq!(root.base().transform.translation.y, 0.0);
}
