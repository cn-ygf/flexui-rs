use flexui::{
    apply_theme, find_by_id, find_by_name, find_mut_by_id, layout_node, Canvas, Color, Context,
    Corners, Font, Point, Rect, Size, Theme, WidgetProperty, WidgetPropertyKey,
};

struct TestCanvas;

impl Canvas for TestCanvas {
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: Color, _width: f32) {}
    fn draw_text(&mut self, _text: &str, _origin: Point, _font: &Font, _color: Color) {}
    fn measure_text(&self, text: &str, font: &Font) -> Size {
        Size::new(
            text.chars().count() as f32 * font.size * 0.55,
            font.size * 1.2,
        )
    }
}

#[test]
fn embedded_gallery_loads_all_included_pages() {
    let mut resources = flexui::ResourceManager::new();
    resources.mount(flexui::ZipProvider::embedded_plain_static(include_bytes!(
        concat!(env!("OUT_DIR"), "/assets.zip")
    )));
    let mut context = Context::new();
    context.register_widget_factory("StatusBadge", |element, _| {
        Ok(Box::new(flexui::Label::new(
            element.attr("text-verbatim").unwrap_or("Status"),
        )))
    });
    let mut doc = flexui::load_window_res(&resources, "gallery.xml", &context).unwrap();
    apply_theme(doc.root.as_mut(), &Theme::light());
    layout_node(
        doc.root.as_mut(),
        Rect::new(0.0, 0.0, 920.0, 640.0),
        &TestCanvas,
    );
    let pages = find_by_id(
        doc.root.as_ref(),
        find_by_name(doc.root.as_ref(), "pages").unwrap(),
    )
    .unwrap();
    assert_eq!(pages.base().children.len(), 9);
    for name in [
        "apply_bilibili_theme",
        "restore_default_theme",
        "open_drawn_menu",
        "open_xml_native_menu",
        "open_rust_native_menu",
        "native_context_target",
        "nav_async",
        "http_url",
        "http_go",
        "http_response",
        "nav_virtual_list",
        "virtual_table",
        "virtual_add_column",
        "virtual_remove_column",
        "virtual_add_rows",
        "virtual_delete_selected",
        "virtual_density",
        "virtual_reset",
        "virtual_status",
        "nav_advanced",
        "advanced_custom_badge",
        "advanced_tree",
        "advanced_calendar",
        "advanced_date_picker",
        "advanced_time_picker",
        "advanced_datetime_picker",
        "advanced_mixed_list",
        "advanced_item_action",
    ] {
        assert!(find_by_name(doc.root.as_ref(), name).is_some(), "{name}");
    }
    for name in flexui_gallery_animation_controls() {
        assert!(find_by_name(doc.root.as_ref(), name).is_some(), "{name}");
    }
    let transition_panel = find_by_id(
        doc.root.as_ref(),
        find_by_name(doc.root.as_ref(), "animation_transition_panel").unwrap(),
    )
    .unwrap();
    let transition = transition_panel
        .base()
        .transition
        .expect("动画示例应声明显隐过渡");
    assert_eq!(transition.edge(), flexui::TransitionEdge::Bottom);
    assert_eq!(transition.easing_curve(), flexui::Easing::EaseOut);
    let bilibili_button = find_by_id(
        doc.root.as_ref(),
        find_by_name(doc.root.as_ref(), "apply_bilibili_theme").unwrap(),
    )
    .unwrap();
    assert_eq!(
        bilibili_button.base().resolved_style().bg_color,
        Some(Color::from_u8(251, 114, 153, 255))
    );
    assert_eq!(
        find_by_id(
            doc.root.as_ref(),
            find_by_name(doc.root.as_ref(), "theme_switch").unwrap(),
        )
        .unwrap()
        .base()
        .kind,
        flexui::WidgetKind::Switch
    );
    let left_icon_button = find_by_id(
        doc.root.as_ref(),
        find_by_name(doc.root.as_ref(), "left_icon_text_button").unwrap(),
    )
    .unwrap();
    assert!(matches!(
        left_icon_button.property(WidgetPropertyKey::Icon),
        Some(WidgetProperty::Icon(Some(_)))
    ));
    assert!(matches!(
        left_icon_button.property(WidgetPropertyKey::IconRect),
        Some(WidgetProperty::IconRect(Some(rect)))
            if rect == Rect::new(14.0, 10.0, 20.0, 20.0)
    ));
    assert!(matches!(
        left_icon_button.property(WidgetPropertyKey::TextRect),
        Some(WidgetProperty::TextRect(Some(rect)))
            if rect == Rect::new(46.0, 0.0, 96.0, 40.0)
    ));
    for name in ["nav_basic", "nav_forms", "default_button", "primary_button"] {
        let widget = find_by_id(
            doc.root.as_ref(),
            find_by_name(doc.root.as_ref(), name).unwrap(),
        )
        .unwrap();
        assert!(widget.base().rect.size.width > 0.0, "{name} width");
        assert!(widget.base().rect.size.height > 0.0, "{name} height");
        assert!(
            widget.base().resolved_style().fg_color.is_some(),
            "{name} text color"
        );
    }

    let pages_id = find_by_name(doc.root.as_ref(), "pages").unwrap();
    assert!(find_mut_by_id(doc.root.as_mut(), pages_id)
        .unwrap()
        .set_selected_index(8));
    layout_node(
        doc.root.as_mut(),
        Rect::new(0.0, 0.0, 920.0, 640.0),
        &TestCanvas,
    );
    for name in [
        "advanced_xml_template",
        "advanced_tree",
        "advanced_calendar",
        "advanced_mixed_list",
        "advanced_item_action",
    ] {
        let widget = find_by_id(
            doc.root.as_ref(),
            find_by_name(doc.root.as_ref(), name).unwrap(),
        )
        .unwrap();
        assert!(widget.base().rect.size.width > 0.0, "{name} width");
        assert!(widget.base().rect.size.height > 0.0, "{name} height");
    }
}

fn flexui_gallery_animation_controls() -> &'static [&'static str] {
    &[
        "nav_animations",
        "animation_compare",
        "animation_reset_easing",
        "animation_linear",
        "animation_ease_in",
        "animation_ease_out",
        "animation_ease_in_out",
        "animation_transition_panel",
        "animation_show_transition",
        "animation_hide_transition",
        "animation_toggle_transition",
        "animation_progress",
        "animation_progress_toggle",
    ]
}
