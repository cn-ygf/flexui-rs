//! 系统原生弹出菜单的跨平台描述。
//!
//! 本模块只描述菜单命令树和弹出锚点；AppKit/Win32 对象由平台后端临时创建。

use crate::{ImageSource, Point};

/// 系统原生弹出菜单。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NativeMenu {
    pub items: Vec<NativeMenuEntry>,
}

impl NativeMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_items(items: impl IntoIterator<Item = NativeMenuEntry>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }

    pub fn push(&mut self, entry: impl Into<NativeMenuEntry>) {
        self.items.push(entry.into());
    }

    pub fn item(mut self, item: NativeMenuItem) -> Self {
        self.push(item);
        self
    }

    pub fn separator(mut self) -> Self {
        self.push(NativeMenuEntry::Separator);
        self
    }

    pub fn submenu(mut self, submenu: NativeSubmenu) -> Self {
        self.push(submenu);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// 原生菜单中的一项。
#[derive(Debug, Clone, PartialEq)]
pub enum NativeMenuEntry {
    Item(NativeMenuItem),
    Separator,
    Submenu(NativeSubmenu),
}

impl From<NativeMenuItem> for NativeMenuEntry {
    fn from(value: NativeMenuItem) -> Self {
        Self::Item(value)
    }
}

impl From<NativeSubmenu> for NativeMenuEntry {
    fn from(value: NativeSubmenu) -> Self {
        Self::Submenu(value)
    }
}

/// 可被选择的菜单命令。
#[derive(Debug, Clone, PartialEq)]
pub struct NativeMenuItem {
    /// 选择后返回给业务层的稳定命令 ID。
    pub id: String,
    pub text: String,
    pub enabled: bool,
    pub checked: bool,
    /// 跨平台快捷键字符串，例如 `CmdOrCtrl+N`、`Shift+F10`。
    pub shortcut: Option<String>,
    pub icon: Option<ImageSource>,
}

impl NativeMenuItem {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            enabled: true,
            checked: false,
            shortcut: None,
            icon: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn icon(mut self, icon: ImageSource) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// 带子项的原生子菜单。
#[derive(Debug, Clone, PartialEq)]
pub struct NativeSubmenu {
    pub text: String,
    pub enabled: bool,
    pub icon: Option<ImageSource>,
    pub items: Vec<NativeMenuEntry>,
}

impl NativeSubmenu {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            enabled: true,
            icon: None,
            items: Vec::new(),
        }
    }

    pub fn with_items(
        text: impl Into<String>,
        items: impl IntoIterator<Item = NativeMenuEntry>,
    ) -> Self {
        Self {
            text: text.into(),
            enabled: true,
            icon: None,
            items: items.into_iter().collect(),
        }
    }

    pub fn push(&mut self, entry: impl Into<NativeMenuEntry>) {
        self.items.push(entry.into());
    }

    pub fn item(mut self, item: NativeMenuItem) -> Self {
        self.push(item);
        self
    }

    pub fn separator(mut self) -> Self {
        self.push(NativeMenuEntry::Separator);
        self
    }

    pub fn submenu(mut self, submenu: NativeSubmenu) -> Self {
        self.push(submenu);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn icon(mut self, icon: ImageSource) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// 业务层可使用的弹出位置。
#[derive(Debug, Clone, PartialEq)]
pub enum NativeMenuAnchor {
    /// 使用系统当前光标位置，适合窗口右键和托盘回调。
    Cursor,
    /// 以具名控件底部左侧为锚点。
    Control(String),
    /// 窗口内容区逻辑坐标，左上为原点。
    Window(Point),
    /// 桌面屏幕坐标，左上为原点；供托盘等窗口外宿主复用。
    Screen(Point),
}

/// 已去除控件依赖、交给平台后端的弹出位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeMenuPopupAnchor {
    Cursor,
    Window(Point),
    Screen(Point),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 代码可构建完整菜单树() {
        let menu = NativeMenu::new()
            .item(NativeMenuItem::new("new", "New").shortcut("CmdOrCtrl+N"))
            .separator()
            .submenu(
                NativeSubmenu::new("Share")
                    .item(NativeMenuItem::new("copy_link", "Copy link"))
                    .item(NativeMenuItem::new("disabled", "Disabled").enabled(false)),
            );
        assert_eq!(menu.items.len(), 3);
        assert!(matches!(menu.items[1], NativeMenuEntry::Separator));
    }
}
