use flexui::{
    build_window, Color, ImageFit, Insets, MenuAlignment, MenuEntry, MenuStyle, Rect, Size, Skin,
    WindowCtx, WindowImpl,
};

use crate::login::LoginDialog;
use crate::resources::{original_bitmap, original_svg, resources};
use crate::settings::SettingsDialog;

pub(crate) struct MainWindow;

impl WindowImpl for MainWindow {
    fn skin(&self) -> Skin {
        Skin::res("app.xml")
    }

    fn resources(&self) -> flexui::ResourceManager {
        resources()
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "open_login" => {
                if let Ok(dialog) = build_window(LoginDialog::default()) {
                    ctx.open_modal(dialog);
                }
            }
            "open_settings" => {
                let anchor = ctx
                    .with("open_settings", |widget| widget.base().rect)
                    .unwrap_or(Rect::new(908.0, 2.0, 24.0, 24.0));
                ctx.open_styled_menu_entries(
                    anchor,
                    settings_menu_entries(ctx),
                    settings_menu_style(),
                );
            }
            "menu_settings" => {
                if let Ok(dialog) = build_window(SettingsDialog) {
                    ctx.open_modal(dialog);
                }
            }
            "minimize" => ctx.minimize(),
            "close_window" => ctx.close(),
            "btn_start" => ctx.set_localized_text("home_status", "app.status.steam_selected"),
            _ => {}
        }
    }
}

fn settings_menu_style() -> MenuStyle {
    let panel = original_bitmap(include_bytes!("../assets/common/tray_menu_bg@2.00x.png"));
    let hover = original_svg(include_bytes!("../assets/menu/menu_item_hover.svg"));
    let submenu = MenuStyle {
        width: Some(172.0),
        row_height: 40.0,
        item_padding: Insets::new(16.0, 0.0, 12.0, 0.0),
        panel_padding: Insets::new(22.0, 30.0, 22.0, 30.0),
        background_image: Some(panel.clone()),
        background_fit: Some(ImageFit::NinePatch(Insets::all(32.0))),
        hot_background_image: Some(hover.clone()),
        hot_background_fit: Some(ImageFit::NinePatch(Insets::all(8.0))),
        text: Color::WHITE,
        hot_text: Color::WHITE,
        border: Color::from_u8(0, 0, 0, 0),
        window_margin: Insets::new(8.0, 8.0, 8.0, 8.0),
        ..Default::default()
    };
    MenuStyle {
        width: Some(172.0),
        row_height: 40.0,
        item_padding: Insets::new(38.0, 0.0, 12.0, 0.0),
        panel_padding: Insets::new(22.0, 30.0, 22.0, 30.0),
        background_image: Some(panel),
        background_fit: Some(ImageFit::NinePatch(Insets::all(32.0))),
        hot_background_image: Some(hover),
        hot_background_fit: Some(ImageFit::NinePatch(Insets::all(8.0))),
        text: Color::WHITE,
        hot_text: Color::WHITE,
        border: Color::from_u8(0, 0, 0, 0),
        icon_size: Size::new(18.0, 18.0),
        icon_inset: 13.0,
        submenu_indicator: Some(original_svg(include_bytes!(
            "../assets/menu/menu_arrow_default.svg"
        ))),
        submenu_indicator_size: Size::new(16.0, 16.0),
        submenu_indicator_inset: 8.0,
        window_margin: Insets::new(8.0, 8.0, 8.0, 8.0),
        alignment: MenuAlignment::End,
        // 原图四周包含阴影留白，让可见面板右边和顶部贴齐按钮锚点。
        offset: flexui::Point::new(22.0, -22.0),
        submenu_align_panel_top: true,
        submenu_style: Some(Box::new(submenu)),
        ..Default::default()
    }
}

fn settings_menu_entries(ctx: &WindowCtx) -> Vec<MenuEntry> {
    let text = |key| ctx.localized_text(key);
    vec![
        MenuEntry::item(text("app.menu.coupon"), "menu_coupon")
            .icon(original_svg(include_bytes!("../assets/menu/coupon.svg"))),
        MenuEntry::submenu(
            text("app.menu.submit_issue"),
            vec![
                MenuEntry::item(text("app.menu.issue_faq"), "menu_issue_faq"),
                MenuEntry::item(text("app.menu.self_repair"), "menu_self_repair"),
                MenuEntry::item(text("app.menu.create_ticket"), "menu_create_ticket"),
            ],
        )
        .icon(original_svg(include_bytes!("../assets/menu/feedback.svg"))),
        MenuEntry::item(text("app.menu.messages"), "menu_messages").icon(original_svg(
            include_bytes!("../assets/menu/information.svg"),
        )),
        MenuEntry::item(text("app.menu.download_records"), "menu_download_records")
            .icon(original_svg(include_bytes!("../assets/menu/download.svg"))),
        MenuEntry::item(text("app.menu.settings"), "menu_settings")
            .icon(original_svg(include_bytes!("../assets/menu/setting.svg"))),
    ]
}
