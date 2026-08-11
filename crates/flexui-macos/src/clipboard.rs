//! macOS 系统剪贴板（NSPasteboard 通用板）文本读写。

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// 写入系统剪贴板文本。
pub fn set_text(s: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let ns = NSString::from_str(s);
    unsafe { pb.setString_forType(&ns, NSPasteboardTypeString) };
}

/// 读取系统剪贴板文本（无文本返回 None）。
pub fn get_text() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    let s = unsafe { pb.stringForType(NSPasteboardTypeString) }?;
    Some(s.to_string())
}
