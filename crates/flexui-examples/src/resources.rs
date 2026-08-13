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

    #[test]
    fn embedded_assets_cover_layout_images_and_localizations() {
        let resources = resources();
        assert!(resources
            .read_string("app.xml")
            .unwrap()
            .contains("<Window"));
        assert!(!resources
            .read("label/uu_logo@2.00x.png")
            .unwrap()
            .is_empty());
        assert!(resources
            .read_string("i18n/zh-Hans.json")
            .unwrap()
            .contains("app.window_title"));
    }
}
