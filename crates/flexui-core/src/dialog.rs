//! 系统文件对话框的平台无关描述（L3）。
//!
//! 具体弹窗由各后端实现（macOS NSOpenPanel/NSSavePanel、Windows GetOpenFileName/
//! SHBrowseForFolder）。门面 `flexui::dialog` 按平台分发，返回选中的路径。

use std::path::PathBuf;

/// 扩展名筛选组：显示名 + 扩展名列表（不含点，如 `["png","jpg"]`）。
#[derive(Debug, Clone)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// 对话框类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// 打开文件。
    OpenFile,
    /// 打开目录。
    OpenDirectory,
    /// 保存文件。
    SaveFile,
    /// 保存到目录（选择一个目录）。
    SaveDirectory,
}

/// 对话框配置：标题、默认目录、默认文件名、扩展名筛选。
#[derive(Debug, Clone, Default)]
pub struct FileDialog {
    pub title: Option<String>,
    pub default_dir: Option<PathBuf>,
    pub default_name: Option<String>,
    pub filters: Vec<FileFilter>,
}

impl FileDialog {
    pub fn new() -> Self {
        Self::default()
    }
    /// 设置标题。
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }
    /// 设置默认打开/保存目录。
    pub fn default_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.default_dir = Some(p.into());
        self
    }
    /// 设置默认文件名（保存文件用）。
    pub fn default_name(mut self, n: impl Into<String>) -> Self {
        self.default_name = Some(n.into());
        self
    }
    /// 追加一个扩展名筛选组（exts 不含点）。
    pub fn filter(mut self, name: impl Into<String>, exts: &[&str]) -> Self {
        self.filters.push(FileFilter {
            name: name.into(),
            extensions: exts.iter().map(|s| s.to_string()).collect(),
        });
        self
    }

    /// 所有筛选组的扩展名去重合集（供只需扩展名列表的后端使用）。
    pub fn all_extensions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in &self.filters {
            for e in &f.extensions {
                if !out.contains(e) {
                    out.push(e.clone());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 对话框_构建与扩展名合集() {
        let d = FileDialog::new()
            .title("打开图片")
            .default_dir("/tmp")
            .default_name("a.png")
            .filter("图片", &["png", "jpg"])
            .filter("全部", &["png"]);
        assert_eq!(d.title.as_deref(), Some("打开图片"));
        assert_eq!(d.default_name.as_deref(), Some("a.png"));
        assert_eq!(d.filters.len(), 2);
        // 去重合集。
        assert_eq!(d.all_extensions(), vec!["png".to_string(), "jpg".to_string()]);
    }
}
