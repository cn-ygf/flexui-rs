//! Windows 系统剪贴板（CF_UNICODETEXT）文本读写。owner 传 null 关联当前任务。

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

/// 写入系统剪贴板文本（UTF-16LE）。
pub fn set_text(s: &str) {
    unsafe {
        let utf16: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = utf16.len() * 2;
        let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if hmem.is_null() {
            return;
        }
        let dst = GlobalLock(hmem) as *mut u16;
        if !dst.is_null() {
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), dst, utf16.len());
            GlobalUnlock(hmem);
        }
        if OpenClipboard(null_mut()) != 0 {
            EmptyClipboard();
            // 成功后剪贴板接管 hmem，不能再释放。
            SetClipboardData(CF_UNICODETEXT as u32, hmem);
            CloseClipboard();
        } else {
            GlobalFree(hmem);
        }
    }
}

/// 读取系统剪贴板文本（无则 None）。
pub fn get_text() -> Option<String> {
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT as u32);
        let result = if !h.is_null() {
            let ptr = GlobalLock(h) as *const u16;
            if ptr.is_null() {
                None
            } else {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                let s = String::from_utf16_lossy(slice);
                GlobalUnlock(h);
                Some(s)
            }
        } else {
            None
        };
        CloseClipboard();
        result
    }
}
