use flexui::{ImageSource, Localizer, ResourceManager, ZipProvider};

const EMBEDDED_ASSETS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/assets.zip"));

pub(crate) fn resources() -> ResourceManager {
    let mut resources = ResourceManager::new();
    resources.mount(ZipProvider::embedded_plain_static(EMBEDDED_ASSETS));
    resources
}

pub(crate) fn install_localizer() {
    let localizer = Localizer::new("zh-Hans").expect("开发语言标签必须有效");
    let resources = resources();
    localizer
        .load_json_res(&resources, "i18n/zh-Hans.json")
        .expect("加载简体中文本地化资源失败");
    localizer
        .load_json_res(&resources, "i18n/en.json")
        .expect("加载英文本地化资源失败");
    localizer
        .load_json_res(&resources, "i18n/zh-Hant.json")
        .expect("加载繁体中文本地化资源失败");
    flexui::set_application_localizer(localizer);
}

pub(crate) fn original_bitmap(bytes: &'static [u8]) -> ImageSource {
    ImageSource::bytes_scaled(bytes.to_vec(), 2.0)
}

pub(crate) fn original_svg(bytes: &'static [u8]) -> ImageSource {
    ImageSource::svg(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui::{layout_node, Context, Corners, Font, Point, Rect, Size, Widget};

    struct TestCanvas;

    impl flexui::Canvas for TestCanvas {
        fn fill_rect(&mut self, _rect: Rect, _color: flexui::Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: flexui::Color, _width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: flexui::Color) {}
        fn stroke_round_rect(
            &mut self,
            _rect: Rect,
            _radius: Corners,
            _color: flexui::Color,
            _width: f32,
        ) {}
        fn draw_text(&mut self, _text: &str, _origin: Point, _font: &Font, _color: flexui::Color) {}
        fn measure_text(&self, text: &str, font: &Font) -> Size {
            Size::new(text.chars().count() as f32 * font.size * 0.5, font.size)
        }
    }

    fn find_widget<'a>(node: &'a dyn Widget, name: &str) -> Option<&'a dyn Widget> {
        if node.base().name.as_deref() == Some(name) {
            return Some(node);
        }
        node.base()
            .children
            .iter()
            .find_map(|child| find_widget(child.as_ref(), name))
    }

    #[test]
    fn embedded_assets_cover_layout_images_and_localizations() {
        let resources = resources();
        assert!(resources
            .read_string("app.xml")
            .unwrap()
            .contains("<Window"));
        for page in ["home", "examples", "cloud-play", "cloud-saves"] {
            assert!(resources
                .read_string(&format!("pages/{page}.xml"))
                .unwrap()
                .contains("<Panel"));
        }
        assert!(!resources
            .read("label/uu_logo@2.00x.png")
            .unwrap()
            .is_empty());
        assert!(resources
            .read_string("i18n/zh-Hans.json")
            .unwrap()
            .contains("app.window_title"));

        let mut window = flexui::load_window_res(&resources, "app.xml", &Context::new()).unwrap();
        layout_node(
            window.root.as_mut(),
            Rect::new(0.0, 0.0, 1000.0, 688.0),
            &TestCanvas,
        );
        assert_eq!(
            find_widget(window.root.as_ref(), "main_pages").unwrap().base().children.len(),
            4
        );
        assert!(find_widget(window.root.as_ref(), "page_home").unwrap().base().visible);
        for page in ["page_examples", "page_cloud_play", "page_cloud_saves"] {
            assert!(!find_widget(window.root.as_ref(), page).unwrap().base().visible);
        }
    }
}
