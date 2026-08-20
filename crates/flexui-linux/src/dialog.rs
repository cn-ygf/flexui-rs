//! 系统文件对话框：调用桌面自带的 zenity（GNOME）/ kdialog（KDE）。
//!
//! 与 macOS(NSOpenPanel)/Windows(GetOpenFileName) 同接口，同步阻塞直到用户选择/取消。

use std::path::PathBuf;
use std::process::Command;

use flexui_core::dialog::{DialogKind, FileDialog};

/// 弹出文件对话框，返回所选路径（取消返回 None）。
pub fn show_dialog(kind: DialogKind, opts: &FileDialog) -> Option<PathBuf> {
    zenity(kind, opts).or_else(|| kdialog(kind, opts))
}

/// zenity 实现（GNOME 系）。
fn zenity(kind: DialogKind, opts: &FileDialog) -> Option<PathBuf> {
    let mut cmd = Command::new("zenity");
    cmd.arg("--file-selection");
    if let Some(title) = &opts.title {
        cmd.arg(format!("--title={title}"));
    }
    match kind {
        DialogKind::OpenFile => {}
        DialogKind::OpenDirectory => {
            cmd.arg("--directory");
        }
        DialogKind::SaveFile => {
            cmd.arg("--save").arg("--confirm-overwrite");
        }
        DialogKind::SaveDirectory => {
            cmd.arg("--directory").arg("--save");
        }
    }
    // 默认路径（目录 + 文件名）。
    if let Some(path) = default_path(opts) {
        cmd.arg(format!("--filename={path}"));
    }
    // 扩展名筛选。
    for f in &opts.filters {
        let pats: Vec<String> = f.extensions.iter().map(|e| format!("*.{e}")).collect();
        if !pats.is_empty() {
            cmd.arg(format!("--file-filter={} | {}", f.name, pats.join(" ")));
        }
    }
    run(cmd)
}

/// kdialog 实现（KDE 系，作为回退）。
fn kdialog(kind: DialogKind, opts: &FileDialog) -> Option<PathBuf> {
    let mut cmd = Command::new("kdialog");
    let start = default_path(opts).unwrap_or_default();
    match kind {
        DialogKind::OpenFile => {
            cmd.arg("--getopenfilename").arg(&start);
        }
        DialogKind::OpenDirectory | DialogKind::SaveDirectory => {
            cmd.arg("--getexistingdirectory").arg(&start);
        }
        DialogKind::SaveFile => {
            cmd.arg("--getsavefilename").arg(&start);
        }
    }
    if let Some(title) = &opts.title {
        cmd.arg("--title").arg(title);
    }
    run(cmd)
}

/// 默认目录 + 文件名拼成起始路径。
fn default_path(opts: &FileDialog) -> Option<String> {
    match (&opts.default_dir, &opts.default_name) {
        (Some(dir), Some(name)) => Some(dir.join(name).to_string_lossy().into_owned()),
        (Some(dir), None) => Some(format!("{}/", dir.to_string_lossy())),
        (None, Some(name)) => Some(name.clone()),
        (None, None) => None,
    }
}

/// 执行命令，成功且有输出则取第一行为路径。
fn run(mut cmd: Command) -> Option<PathBuf> {
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(PathBuf::from(line))
    }
}
