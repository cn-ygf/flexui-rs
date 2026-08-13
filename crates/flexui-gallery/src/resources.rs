use flexui::{ResourceManager, ZipProvider};

const ASSETS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/assets.zip"));

pub(crate) fn resources() -> ResourceManager {
    let mut resources = ResourceManager::new();
    resources.mount(ZipProvider::embedded_plain_static(ASSETS));
    resources
}
