use flexui::{MainProxy, NativeMenu};
use image::{imageops::FilterType, RgbaImage};

pub(crate) const INIT: &str = "app:tray:init";
pub(crate) const SHOW_MAIN: &str = "app:tray:show-main";
pub(crate) const OPEN_MENU: &str = "app:tray:open-menu";
pub(crate) const COMMAND_PREFIX: &str = "app:tray:command:";

#[cfg(target_os = "linux")]
use flexui::{NativeMenuEntry, NativeMenuItem, NativeSubmenu};
#[cfg(target_os = "linux")]
use ksni::blocking::TrayMethods as _;

/// Linux StatusNotifierItem 的运行状态。菜单回调在 ksni 线程执行，只投递消息回 UI 线程。
#[cfg(target_os = "linux")]
struct LinuxTray {
    proxy: MainProxy,
    tooltip: String,
    icon: Vec<ksni::Icon>,
    menu: NativeMenu,
}

#[cfg(target_os = "linux")]
impl LinuxTray {
    fn send_command(&self, command: &str) {
        self.proxy.send(command_message(command));
    }
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "flexui-examples".to_string()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.proxy.send(SHOW_MAIN);
    }

    fn title(&self) -> String {
        self.tooltip.clone()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.tooltip.clone(),
            icon_pixmap: self.icon.clone(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        map_menu_entries(&self.menu.items)
    }
}

/// 保持 ksni 服务存活；丢弃后关闭 D-Bus 托盘服务。
#[cfg(target_os = "linux")]
pub(crate) struct AppTray {
    handle: ksni::blocking::Handle<LinuxTray>,
}

#[cfg(target_os = "linux")]
impl AppTray {
    pub(crate) fn create(
        proxy: MainProxy,
        tooltip: &str,
        menu: NativeMenu,
    ) -> Result<Self, String> {
        let tray = LinuxTray {
            proxy,
            tooltip: tooltip.to_string(),
            icon: vec![linux_tray_icon()?],
            menu,
        };
        let handle = tray
            .spawn()
            .map_err(|error| format!("创建 Linux 系统托盘图标失败: {error}"))?;
        Ok(Self { handle })
    }
}

#[cfg(target_os = "linux")]
impl Drop for AppTray {
    fn drop(&mut self) {
        self.handle.shutdown().wait();
    }
}

#[cfg(target_os = "linux")]
fn command_message(command: &str) -> String {
    format!("{COMMAND_PREFIX}{command}")
}

#[cfg(target_os = "linux")]
fn map_menu_entries(entries: &[NativeMenuEntry]) -> Vec<ksni::MenuItem<LinuxTray>> {
    entries.iter().map(map_menu_entry).collect()
}

#[cfg(target_os = "linux")]
fn map_menu_entry(entry: &NativeMenuEntry) -> ksni::MenuItem<LinuxTray> {
    match entry {
        NativeMenuEntry::Separator => ksni::MenuItem::Separator,
        NativeMenuEntry::Item(item) => map_command(item),
        NativeMenuEntry::Submenu(submenu) => map_submenu(submenu),
    }
}

#[cfg(target_os = "linux")]
fn map_command(item: &NativeMenuItem) -> ksni::MenuItem<LinuxTray> {
    let command = item.id.clone();
    if item.checked {
        ksni::menu::CheckmarkItem {
            label: item.text.clone(),
            enabled: item.enabled,
            checked: true,
            activate: Box::new(move |tray: &mut LinuxTray| tray.send_command(&command)),
            ..Default::default()
        }
        .into()
    } else {
        ksni::menu::StandardItem {
            label: item.text.clone(),
            enabled: item.enabled,
            activate: Box::new(move |tray: &mut LinuxTray| tray.send_command(&command)),
            ..Default::default()
        }
        .into()
    }
}

#[cfg(target_os = "linux")]
fn map_submenu(submenu: &NativeSubmenu) -> ksni::MenuItem<LinuxTray> {
    ksni::menu::SubMenu {
        label: submenu.text.clone(),
        enabled: submenu.enabled,
        submenu: map_menu_entries(&submenu.items),
        ..Default::default()
    }
    .into()
}

#[cfg(target_os = "linux")]
fn linux_tray_icon() -> Result<ksni::Icon, String> {
    let mut data = tray_image()?.into_raw();
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }
    Ok(ksni::Icon {
        width: 32,
        height: 32,
        data,
    })
}

#[cfg(not(target_os = "linux"))]
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// 保持托盘图标的原生资源存活；丢弃后系统会移除图标。
#[cfg(not(target_os = "linux"))]
pub(crate) struct AppTray {
    _icon: TrayIcon,
}

#[cfg(not(target_os = "linux"))]
impl AppTray {
    pub(crate) fn create(
        proxy: MainProxy,
        tooltip: &str,
        _menu: NativeMenu,
    ) -> Result<Self, String> {
        TrayIconEvent::set_event_handler(Some(move |event| {
            let message = match event {
                TrayIconEvent::Click {
                    button,
                    button_state: MouseButtonState::Up,
                    ..
                } => click_message(button),
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } if !cfg!(target_os = "macos") => Some(SHOW_MAIN),
                _ => None,
            };
            if let Some(message) = message {
                proxy.send(message);
            }
        }));

        let icon = tray_icon()?;
        let icon = TrayIconBuilder::new()
            .with_tooltip(tooltip)
            .with_icon(icon)
            .with_icon_as_template(cfg!(target_os = "macos"))
            .with_menu_on_left_click(false)
            .with_menu_on_right_click(false)
            .build()
            .map_err(|error| format!("创建系统托盘图标失败: {error}"))?;
        Ok(Self { _icon: icon })
    }
}

#[cfg(not(target_os = "linux"))]
fn click_message(button: MouseButton) -> Option<&'static str> {
    click_message_for_platform(button, cfg!(target_os = "macos"))
}

#[cfg(not(target_os = "linux"))]
fn click_message_for_platform(button: MouseButton, macos: bool) -> Option<&'static str> {
    if macos {
        matches!(button, MouseButton::Left | MouseButton::Right).then_some(OPEN_MENU)
    } else {
        match button {
            MouseButton::Left => Some(SHOW_MAIN),
            MouseButton::Right => Some(OPEN_MENU),
            MouseButton::Middle => None,
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn tray_icon() -> Result<tray_icon::Icon, String> {
    let canvas = tray_image()?;
    tray_icon::Icon::from_rgba(canvas.into_raw(), 32, 32)
        .map_err(|error| format!("转换托盘图标失败: {error}"))
}

fn tray_image() -> Result<RgbaImage, String> {
    let source = image::load_from_memory(include_bytes!("../assets/label/uu_logo@2.00x.png"))
        .map_err(|error| format!("读取托盘图标失败: {error}"))?
        .to_rgba8();

    // 原图纵向略长，等比缩放后居中，避免状态栏图标被拉宽。
    let resized = image::imageops::resize(&source, 28, 32, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(32, 32);
    image::imageops::overlay(&mut canvas, &resized, 2, 0);
    Ok(canvas)
}

#[cfg(all(test, not(target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn 托盘点击行为按平台区分() {
        assert_eq!(
            click_message_for_platform(MouseButton::Left, true),
            Some(OPEN_MENU)
        );
        assert_eq!(
            click_message_for_platform(MouseButton::Right, true),
            Some(OPEN_MENU)
        );
        assert_eq!(
            click_message_for_platform(MouseButton::Left, false),
            Some(SHOW_MAIN)
        );
        assert_eq!(
            click_message_for_platform(MouseButton::Right, false),
            Some(OPEN_MENU)
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    use flexui::{Dispatcher, NativeSubmenu};

    fn test_tray(proxy: MainProxy, menu: NativeMenu) -> LinuxTray {
        LinuxTray {
            proxy,
            tooltip: "测试托盘".to_string(),
            icon: Vec::new(),
            menu,
        }
    }

    #[test]
    fn 原生菜单递归映射并保留状态() {
        let menu = NativeMenu::new()
            .item(
                NativeMenuItem::new("checked", "已选")
                    .enabled(false)
                    .checked(true),
            )
            .separator()
            .submenu(
                NativeSubmenu::new("更多")
                    .enabled(false)
                    .item(NativeMenuItem::new("nested", "子命令")),
            );
        let dispatcher = Dispatcher::new();
        let mut tray = test_tray(dispatcher.proxy(), menu);
        let mapped = ksni::Tray::menu(&tray);

        assert_eq!(mapped.len(), 3);
        let ksni::MenuItem::Checkmark(checked) = &mapped[0] else {
            panic!("勾选项应映射为 CheckmarkItem");
        };
        assert_eq!(checked.label, "已选");
        assert!(!checked.enabled);
        assert!(checked.checked);
        assert!(matches!(mapped[1], ksni::MenuItem::Separator));
        let ksni::MenuItem::SubMenu(submenu) = &mapped[2] else {
            panic!("子菜单应保持层级");
        };
        assert_eq!(submenu.label, "更多");
        assert!(!submenu.enabled);
        assert_eq!(submenu.submenu.len(), 1);

        let ksni::MenuItem::Standard(nested) = &submenu.submenu[0] else {
            panic!("普通命令应映射为 StandardItem");
        };
        (nested.activate)(&mut tray);
        assert_eq!(
            dispatcher.drain_messages(),
            vec![format!("{COMMAND_PREFIX}nested")]
        );
    }

    #[test]
    fn 左键激活请求显示主窗口() {
        let dispatcher = Dispatcher::new();
        let mut tray = test_tray(dispatcher.proxy(), NativeMenu::new());

        ksni::Tray::activate(&mut tray, 0, 0);

        assert_eq!(dispatcher.drain_messages(), vec![SHOW_MAIN.to_string()]);
    }
}
