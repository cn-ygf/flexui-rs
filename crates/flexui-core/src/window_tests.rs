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
fn 窗口配置可用代码设置初始居中() {
    let config = WindowConfig::new("center", 320.0, 240.0).centered();
    assert_eq!(config.initial_position, WindowInitialPosition::CenterScreen);
}

#[test]
fn 窗口高级属性支持纯代码配置() {
    let config = WindowConfig::new("advanced", 640.0, 480.0)
        .position(120, 80)
        .min_size(320.0, 240.0)
        .max_size(1280.0, 960.0)
        .opacity(0.8)
        .always_on_top(true)
        .no_activate(true)
        .show_in_taskbar(false)
        .fullscreen(true);
    assert_eq!(
        config.initial_position,
        WindowInitialPosition::Position { x: 120, y: 80 }
    );
    assert_eq!(
        (config.min_width, config.min_height),
        (Some(320.0), Some(240.0))
    );
    assert_eq!(
        (config.max_width, config.max_height),
        (Some(1280.0), Some(960.0))
    );
    assert_eq!(config.opacity, 0.8);
    assert!(config.always_on_top && config.no_activate && config.fullscreen);
    assert!(!config.show_in_taskbar);
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
