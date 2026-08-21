//! 系统剪贴板（X11 selection / Wayland data-control，经 arboard）。
//!
//! arboard 的 `Clipboard` 在 X11 下用后台线程持有 selection，必须保活整个进程，
//! 因此放全局；`clipboard::{get_text, set_text}` 与 macOS/Windows 后端同接口。

use std::sync::{Mutex, OnceLock};

use arboard::Clipboard;

fn instance() -> &'static Mutex<Option<Clipboard>> {
    static CLIP: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();
    CLIP.get_or_init(|| Mutex::new(Clipboard::new().ok()))
}

/// 读取剪贴板文本（无则 None）。
pub fn get_text() -> Option<String> {
    let mut guard = instance().lock().ok()?;
    guard.as_mut()?.get_text().ok()
}

/// 写入剪贴板文本。
pub fn set_text(text: &str) {
    if let Ok(mut guard) = instance().lock() {
        if let Some(clip) = guard.as_mut() {
            let _ = clip.set_text(text.to_owned());
        }
    }
}
