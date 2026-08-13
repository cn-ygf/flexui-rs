//! Windows 文件对话框：GetOpenFileName/GetSaveFileName（文件）+ SHBrowseForFolder（目录）。

use std::path::PathBuf;
use std::ptr::{null, null_mut};

use flexui_core::dialog::{DialogKind, FileDialog};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
    OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Shell::{
    SHBrowseForFolderW, SHGetPathFromIDListW, BFFM_INITIALIZED, BFFM_SETSELECTIONW,
    BIF_RETURNONLYFSDIRS, BROWSEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

/// 弹出系统文件对话框（模态），返回选中的路径；取消返回 None。
pub fn show_dialog(kind: DialogKind, opts: &FileDialog) -> Option<PathBuf> {
    unsafe {
        match kind {
            DialogKind::OpenFile => file_dialog(false, opts),
            DialogKind::SaveFile => file_dialog(true, opts),
            DialogKind::OpenDirectory | DialogKind::SaveDirectory => folder_dialog(opts),
        }
    }
}

/// UTF-8 → NUL 结尾 UTF-16。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 从 NUL 结尾 UTF-16 缓冲读出字符串。
fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// 构造 comdlg32 的双 NUL 结尾筛选串："名\0*.a;*.b\0...\0"。
fn build_filter(opts: &FileDialog) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    let mut push = |s: &str| {
        out.extend(s.encode_utf16());
        out.push(0);
    };
    if opts.filters.is_empty() {
        push("所有文件");
        push("*.*");
    } else {
        for f in &opts.filters {
            push(&f.name);
            let pat = if f.extensions.is_empty() {
                "*.*".to_string()
            } else {
                f.extensions
                    .iter()
                    .map(|e| format!("*.{e}"))
                    .collect::<Vec<_>>()
                    .join(";")
            };
            push(&pat);
        }
    }
    out.push(0); // 结尾再补一个 NUL（双 NUL 结束）
    out
}

/// 打开/保存文件对话框。
unsafe fn file_dialog(save: bool, opts: &FileDialog) -> Option<PathBuf> {
    let mut file_buf = vec![0u16; 4096];
    // 保存时预填默认文件名。
    if save {
        if let Some(name) = &opts.default_name {
            let w = wide(name);
            let n = w.len().min(file_buf.len());
            file_buf[..n].copy_from_slice(&w[..n]);
        }
    }
    let filter = build_filter(opts);
    let title = opts.title.as_ref().map(|t| wide(t));
    let initdir = opts
        .default_dir
        .as_ref()
        .map(|d| wide(&d.to_string_lossy()));

    let mut ofn: OPENFILENAMEW = std::mem::zeroed();
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file_buf.as_mut_ptr();
    ofn.nMaxFile = file_buf.len() as u32;
    ofn.lpstrTitle = title.as_ref().map(|t| t.as_ptr()).unwrap_or(null());
    ofn.lpstrInitialDir = initdir.as_ref().map(|d| d.as_ptr()).unwrap_or(null());
    ofn.Flags = OFN_EXPLORER
        | OFN_NOCHANGEDIR
        | OFN_PATHMUSTEXIST
        | if save {
            OFN_OVERWRITEPROMPT
        } else {
            OFN_FILEMUSTEXIST
        };

    let ok = if save {
        GetSaveFileNameW(&mut ofn)
    } else {
        GetOpenFileNameW(&mut ofn)
    };
    if ok == 0 {
        return None;
    }
    Some(PathBuf::from(from_wide(&file_buf)))
}

/// 目录选择回调：初始化时把选中项定位到默认目录。
unsafe extern "system" fn browse_cb(hwnd: HWND, msg: u32, _lp: LPARAM, data: LPARAM) -> i32 {
    if msg == BFFM_INITIALIZED && data != 0 {
        // data 为默认路径（宽字符串）指针；wParam=TRUE 表示 lParam 是路径串。
        SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, data);
    }
    0
}

/// 目录选择对话框（经典样式，无需 OLE 初始化）。
unsafe fn folder_dialog(opts: &FileDialog) -> Option<PathBuf> {
    let title = opts.title.as_ref().map(|t| wide(t));
    let default = opts
        .default_dir
        .as_ref()
        .map(|d| wide(&d.to_string_lossy()));
    let mut display = vec![0u16; 260];

    let mut bi: BROWSEINFOW = std::mem::zeroed();
    bi.hwndOwner = null_mut();
    bi.pszDisplayName = display.as_mut_ptr();
    bi.lpszTitle = title.as_ref().map(|t| t.as_ptr()).unwrap_or(null());
    bi.ulFlags = BIF_RETURNONLYFSDIRS;
    if let Some(d) = &default {
        bi.lpfn = Some(browse_cb);
        bi.lParam = d.as_ptr() as LPARAM;
    }

    let pidl = SHBrowseForFolderW(&bi);
    if pidl.is_null() {
        return None;
    }
    let mut path = vec![0u16; 260];
    let got = SHGetPathFromIDListW(pidl, path.as_mut_ptr());
    CoTaskMemFree(pidl as *const core::ffi::c_void);
    if got == 0 {
        None
    } else {
        Some(PathBuf::from(from_wide(&path)))
    }
}
