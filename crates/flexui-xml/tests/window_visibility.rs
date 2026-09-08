use flexui_core::WindowConfig;
use flexui_xml::{load_window_str, Context};

#[test]
fn 代码窗口配置默认显示且可设为初始隐藏() {
    assert!(WindowConfig::default().visible);
    assert!(
        !WindowConfig::new("隐藏窗口", 320.0, 240.0)
            .visible(false)
            .visible
    );
}

#[test]
fn xml窗口支持初始隐藏() {
    let doc = load_window_str(
        r#"<Window title="隐藏窗口" visible="false"><Panel/></Window>"#,
        &Context::new(),
    )
    .expect("窗口 XML 应可解析");
    assert!(!doc.config.expect("应解析窗口配置").visible);
}
