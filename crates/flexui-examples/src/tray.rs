use flexui::MainProxy;
use image::{imageops::FilterType, RgbaImage};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub(crate) const INIT: &str = "app:tray:init";
pub(crate) const SHOW_MAIN: &str = "app:tray:show-main";
pub(crate) const OPEN_MENU: &str = "app:tray:open-menu";

/// 保持托盘图标的原生资源存活；丢弃后系统会移除图标。
pub(crate) struct AppTray {
    _icon: TrayIcon,
}

impl AppTray {
    pub(crate) fn create(proxy: MainProxy, tooltip: &str) -> Result<Self, String> {
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

fn click_message(button: MouseButton) -> Option<&'static str> {
    click_message_for_platform(button, cfg!(target_os = "macos"))
}

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

fn tray_icon() -> Result<tray_icon::Icon, String> {
    let source = image::load_from_memory(include_bytes!("../assets/label/uu_logo@2.00x.png"))
        .map_err(|error| format!("读取托盘图标失败: {error}"))?
        .to_rgba8();

    // 原图纵向略长，等比缩放后居中，避免状态栏图标被拉宽。
    let resized = image::imageops::resize(&source, 28, 32, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(32, 32);
    image::imageops::overlay(&mut canvas, &resized, 2, 0);
    tray_icon::Icon::from_rgba(canvas.into_raw(), 32, 32)
        .map_err(|error| format!("转换托盘图标失败: {error}"))
}

#[cfg(test)]
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
