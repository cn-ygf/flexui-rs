//! FlexUI 原生菜单模型到 AppKit/Win32 菜单对象的内部适配层。
//!
//! 基于 muda，仅服务 macOS/Windows；Linux 后端用自绘 Cairo 菜单，本 crate 在
//! Linux 上编译为空壳（不引入 muda/GTK），以便 `cargo build --workspace` 通过。
#![cfg(not(target_os = "linux"))]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use flexui_core::{ImageSource, NativeMenu, NativeMenuEntry, NativeSubmenu};
use image::imageops::FilterType;
use muda::{
    accelerator::Accelerator, CheckMenuItem, ContextMenu, Icon, IconMenuItem, IsMenuItem, Menu,
    MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};

static NEXT_MENU_ID: AtomicU64 = AtomicU64::new(1);

/// 在有效 NSView 上弹出菜单。位置为视图逻辑坐标；None 使用光标位置。
///
/// # Safety
/// `view` 必须是有效、存活的 `NSView*`（由调用方保证生命周期）；否则 AppKit 侧解引用 UB。
#[cfg(target_os = "macos")]
pub unsafe fn popup_for_nsview(
    menu: &NativeMenu,
    view: *const std::ffi::c_void,
    position: Option<(f64, f64)>,
) -> Option<String> {
    let prepared = PreparedMenu::new(menu)?;
    clear_events();
    let position = position.map(|point| muda::dpi::LogicalPosition::new(point.0, point.1).into());
    if !unsafe { prepared.native.show_context_menu_for_nsview(view, position) } {
        return None;
    }
    prepared.selected_command()
}

/// 在有效 HWND 上弹出菜单。位置为客户区逻辑坐标；None 使用光标位置。
///
/// # Safety
/// `hwnd` 必须是有效、存活的窗口句柄（由调用方保证生命周期）；否则 Win32 侧解引用 UB。
#[cfg(target_os = "windows")]
pub unsafe fn popup_for_hwnd(
    menu: &NativeMenu,
    hwnd: isize,
    position: Option<(f64, f64)>,
) -> Option<String> {
    let prepared = PreparedMenu::new(menu)?;
    clear_events();
    let position = position.map(|point| muda::dpi::LogicalPosition::new(point.0, point.1).into());
    if !unsafe { prepared.native.show_context_menu_for_hwnd(hwnd, position) } {
        return None;
    }
    prepared.selected_command()
}

struct PreparedMenu {
    native: Menu,
    commands: HashMap<String, String>,
}

impl PreparedMenu {
    fn new(menu: &NativeMenu) -> Option<Self> {
        let native = Menu::new();
        let prefix = format!(
            "flexui-native-{}-",
            NEXT_MENU_ID.fetch_add(1, Ordering::Relaxed)
        );
        let mut commands = HashMap::new();
        append_entries(&menu.items, &prefix, &mut commands, |item| {
            native.append(item)
        })
        .ok()?;
        Some(Self { native, commands })
    }

    fn selected_command(&self) -> Option<String> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(command) = self.commands.get(event.id().as_ref()) {
                return Some(command.clone());
            }
        }
        None
    }
}

fn clear_events() {
    while MenuEvent::receiver().try_recv().is_ok() {}
}

fn append_entries(
    entries: &[NativeMenuEntry],
    prefix: &str,
    commands: &mut HashMap<String, String>,
    append: impl Fn(&dyn IsMenuItem) -> muda::Result<()>,
) -> muda::Result<()> {
    for (index, entry) in entries.iter().enumerate() {
        match entry {
            NativeMenuEntry::Separator => append(&PredefinedMenuItem::separator())?,
            NativeMenuEntry::Item(item) => {
                let native_id = format!("{prefix}{index}-{}", item.id);
                let accelerator = parse_accelerator(item.shortcut.as_deref());
                if item.checked {
                    append(&CheckMenuItem::with_id(
                        &native_id,
                        &item.text,
                        item.enabled,
                        true,
                        accelerator,
                    ))?;
                } else if let Some(icon) = item.icon.as_ref().and_then(load_icon) {
                    append(&IconMenuItem::with_id(
                        &native_id,
                        &item.text,
                        item.enabled,
                        Some(icon),
                        accelerator,
                    ))?;
                } else {
                    append(&MenuItem::with_id(
                        &native_id,
                        &item.text,
                        item.enabled,
                        accelerator,
                    ))?;
                }
                commands.insert(native_id, item.id.clone());
            }
            NativeMenuEntry::Submenu(submenu) => {
                let native_submenu =
                    build_submenu(submenu, &format!("{prefix}{index}-"), commands)?;
                append(&native_submenu)?;
            }
        }
    }
    Ok(())
}

fn build_submenu(
    submenu: &NativeSubmenu,
    prefix: &str,
    commands: &mut HashMap<String, String>,
) -> muda::Result<Submenu> {
    let native = Submenu::new(&submenu.text, submenu.enabled);
    if let Some(icon) = submenu.icon.as_ref().and_then(load_icon) {
        native.set_icon(Some(icon));
    }
    append_entries(&submenu.items, prefix, commands, |item| native.append(item))?;
    Ok(native)
}

fn parse_accelerator(value: Option<&str>) -> Option<Accelerator> {
    value.and_then(|value| match value.parse() {
        Ok(accelerator) => Some(accelerator),
        Err(error) => {
            eprintln!("[flexui] 忽略无效菜单快捷键 {value:?}: {error}");
            None
        }
    })
}

fn load_icon(source: &ImageSource) -> Option<Icon> {
    let rgba = match source {
        ImageSource::Svg(bytes) => flexui_svg::rasterize(bytes, 18, 18)?,
        ImageSource::Path(path) | ImageSource::ScaledPath(path, _) => {
            decode_bitmap(&std::fs::read(path).ok()?)?
        }
        ImageSource::Bytes(bytes) | ImageSource::ScaledBytes(bytes, _) => decode_bitmap(bytes)?,
    };
    Icon::from_rgba(rgba, 18, 18).ok()
}

fn decode_bitmap(bytes: &[u8]) -> Option<Vec<u8>> {
    Some(
        image::load_from_memory(bytes)
            .ok()?
            .resize_exact(18, 18, FilterType::Lanczos3)
            .to_rgba8()
            .into_raw(),
    )
}
