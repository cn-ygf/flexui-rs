use flexui::{
    apply_theme, find_by_id, find_by_name, layout_node, Canvas, Color, Context, Corners, Font,
    Point, Rect, Size, Theme,
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
    let mut doc = flexui::load_window_res(&resources, "gallery.xml", &Context::new()).unwrap();
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
    assert_eq!(pages.base().children.len(), 4);
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
}
