//! macOS 文件对话框：NSOpenPanel / NSSavePanel 封装。

use std::path::PathBuf;

use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSSavePanel};
use objc2_foundation::{MainThreadMarker, NSArray, NSString, NSURL};

use flexui_core::dialog::{DialogKind, FileDialog};

/// 弹出系统文件对话框（模态），返回选中的路径。取消返回 None。
///
/// 必须在主线程调用（UI 回调即主线程）。
pub fn show_dialog(kind: DialogKind, opts: &FileDialog) -> Option<PathBuf> {
    let mtm = MainThreadMarker::new()?;
    match kind {
        DialogKind::SaveFile => {
            let panel = NSSavePanel::savePanel(mtm);
            apply_common(&panel, opts);
            if let Some(name) = &opts.default_name {
                panel.setNameFieldStringValue(&NSString::from_str(name));
            }
            set_allowed_types(&panel, opts);
            if panel.runModal() == NSModalResponseOK {
                url_path(panel.URL().as_deref())
            } else {
                None
            }
        }
        DialogKind::OpenFile | DialogKind::OpenDirectory | DialogKind::SaveDirectory => {
            let panel = NSOpenPanel::openPanel(mtm);
            let want_files = kind == DialogKind::OpenFile;
            panel.setCanChooseFiles(want_files);
            panel.setCanChooseDirectories(!want_files);
            panel.setAllowsMultipleSelection(false);
            // NSOpenPanel 继承自 NSSavePanel，共用标题/目录设置。
            apply_common(&panel, opts);
            if want_files {
                set_allowed_types(&panel, opts);
            }
            if panel.runModal() == NSModalResponseOK {
                url_path(panel.URL().as_deref())
            } else {
                None
            }
        }
    }
}

/// 设置标题与默认目录（NSSavePanel/NSOpenPanel 共用）。
fn apply_common(panel: &NSSavePanel, opts: &FileDialog) {
    if let Some(t) = &opts.title {
        panel.setTitle(Some(&NSString::from_str(t)));
    }
    if let Some(dir) = &opts.default_dir {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
        panel.setDirectoryURL(Some(&url));
    }
}

/// 用扩展名合集限制可选文件类型（无筛选则不限制）。
fn set_allowed_types(panel: &NSSavePanel, opts: &FileDialog) {
    let exts = opts.all_extensions();
    if exts.is_empty() {
        return;
    }
    let ns: Vec<_> = exts.iter().map(|e| NSString::from_str(e)).collect();
    let refs: Vec<&NSString> = ns.iter().map(|s| s.as_ref()).collect();
    let arr = NSArray::from_slice(&refs);
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&arr));
}

/// NSURL → 文件系统路径。
fn url_path(url: Option<&NSURL>) -> Option<PathBuf> {
    let p = url?.path()?;
    Some(PathBuf::from(p.to_string()))
}
