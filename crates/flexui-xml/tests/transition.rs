use flexui_core::{Easing, TransitionEdge};
use flexui_xml::{load_str, Context};

#[test]
fn xml_可声明底部滑动显隐过渡() {
    let loaded = load_str(
        r#"<Box transition="slide-bottom" transition-distance="440"
            transition-duration="0.22" transition-easing="ease-out"/>"#,
        &Context::new(),
    )
    .expect("XML 应能加载显隐过渡");
    let transition = loaded.root.base().transition.expect("应生成 Transition");

    assert_eq!(transition.edge(), TransitionEdge::Bottom);
    assert_eq!(transition.distance(), 440.0);
    assert!((transition.duration_secs() - 0.22).abs() < f32::EPSILON);
    assert_eq!(transition.easing_curve(), Easing::EaseOut);
}

#[test]
fn xml_显隐过渡必须声明有效距离() {
    let error = load_str(
        r#"<Box transition="slide-bottom" transition-distance="0"/>"#,
        &Context::new(),
    )
    .err()
    .expect("无效距离应拒绝加载");

    assert!(error.to_string().contains("transition-distance 必须大于 0"));
}
