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
    use flexui::{
        apply_theme, layout_node, Color, Context, Corners, Font, Point, Rect, Size, Sizing, Theme,
        Widget,
    };

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
        ) {
        }
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

    fn load_themed_window(path: &str) -> flexui::WindowDoc {
        let resources = resources();
        let mut window = flexui::load_window_res(&resources, path, &Context::new()).unwrap();
        apply_theme(window.root.as_mut(), &Theme::light());
        window
    }

    #[test]
    fn embedded_assets_cover_layout_images_and_localizations() {
        let resources = resources();
        assert!(resources
            .read_string("app.xml")
            .unwrap()
            .contains("<Window"));
        assert!(resources
            .read_string("close_prompt.xml")
            .unwrap()
            .contains("confirm_close"));
        for page in ["home", "examples", "cloud-play", "cloud-saves"] {
            let xml = resources.read_string(&format!("pages/{page}.xml")).unwrap();
            assert!(xml.contains("page_"));
        }
        assert!(!resources
            .read("label/uu_logo@2.00x.png")
            .unwrap()
            .is_empty());
        for frame in 1..=25 {
            assert!(!resources
                .read(&format!("animations/btn_loading/loading_{frame}@2.00x.png"))
                .unwrap()
                .is_empty());
        }
        assert!(resources
            .read_string("i18n/zh-Hans.json")
            .unwrap()
            .contains("app.window_title"));

        let mut window = flexui::load_window_res(&resources, "app.xml", &Context::new()).unwrap();
        apply_theme(window.root.as_mut(), &Theme::light());
        layout_node(
            window.root.as_mut(),
            Rect::new(0.0, 0.0, 1000.0, 688.0),
            &TestCanvas,
        );
        assert_eq!(
            find_widget(window.root.as_ref(), "main_pages")
                .unwrap()
                .base()
                .children
                .len(),
            4
        );
        assert!(
            find_widget(window.root.as_ref(), "page_home")
                .unwrap()
                .base()
                .visible
        );
        for page in ["page_examples", "page_cloud_play", "page_cloud_saves"] {
            assert!(
                !find_widget(window.root.as_ref(), page)
                    .unwrap()
                    .base()
                    .visible
            );
        }
        for button in ["open_settings", "open_login"] {
            let style = find_widget(window.root.as_ref(), button)
                .unwrap()
                .base()
                .resolved_style();
            assert!(style.bg_image.is_some(), "{button} background image");
            assert_eq!(style.bg_color, None, "{button} background color");
            assert_eq!(style.border_color, None, "{button} border color");
            assert_eq!(style.border_width, None, "{button} border width");
        }
    }

    #[test]
    fn 登录输入区的透明控件不继承默认主题背景() {
        let window = load_themed_window("login.xml");
        for name in ["nation_code_text", "edit_phone", "edit_code", "get_code"] {
            let style = find_widget(window.root.as_ref(), name)
                .unwrap()
                .base()
                .resolved_style();
            assert_eq!(
                style.bg_color,
                Some(Color::from_u8(0, 0, 0, 0)),
                "{name} background color"
            );
            assert_eq!(style.border_width, Some(0.0), "{name} border width");
        }
    }

    #[test]
    fn 系统设置布局尺寸与原版皮肤一致() {
        let window = load_themed_window("settings.xml");
        let root = window.root.as_ref();

        let heading = find_widget(root, "settings_heading").unwrap().base();
        assert_eq!(heading.font.size, 16.0);
        assert!(heading.font.bold);

        let body = find_widget(root, "startup_label").unwrap().base();
        assert_eq!(body.font.size, 14.0);

        let detail = find_widget(root, "keep_network_detail").unwrap().base();
        assert_eq!(detail.font.size, 12.0);

        let language = find_widget(root, "language").unwrap().base();
        assert_eq!(language.width, Sizing::Fixed(144.0));
        assert_eq!(language.height, Sizing::Fixed(28.0));

        for name in [
            "set_startup",
            "set_desktop",
            "set_auto",
            "set_sleep",
            "set_audio",
            "set_notice",
        ] {
            let switch = find_widget(root, name).unwrap().base();
            assert_eq!(switch.width, Sizing::Fixed(30.0), "{name} width");
            assert_eq!(switch.height, Sizing::Fixed(16.0), "{name} height");
        }
    }

    #[test]
    fn 关闭提示布局尺寸与原版皮肤一致() {
        let mut window = load_themed_window("close_prompt.xml");
        let config = window.config.as_ref().unwrap();
        assert_eq!(config.width, 360.0);
        assert_eq!(config.height, 210.0);

        layout_node(
            window.root.as_mut(),
            Rect::new(0.0, 0.0, 360.0, 210.0),
            &TestCanvas,
        );
        let confirm = find_widget(window.root.as_ref(), "confirm_close")
            .unwrap()
            .base();
        assert_eq!(confirm.width, Sizing::Fixed(120.0));
        assert_eq!(confirm.height, Sizing::Fixed(36.0));
        assert_eq!(confirm.resolved_style().border_width, Some(0.0));

        let checkbox = find_widget(window.root.as_ref(), "close_remember")
            .unwrap()
            .base();
        let checkbox_center = checkbox.rect.left() + checkbox.rect.size.width / 2.0;
        assert!(
            (checkbox_center - 180.0).abs() < 0.01,
            "checkbox center: {checkbox_center}"
        );
    }
}
