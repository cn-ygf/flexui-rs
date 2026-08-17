use flexui::{
    build_window, Color, ImageFit, Insets, MenuAlignment, MenuEntry, MenuStyle, NativeMenu,
    NativeMenuAnchor, NativeMenuItem, Rect, Size, Skin, WindowCtx, WindowImpl,
};

use crate::close_prompt::{
    ClosePrompt, CLOSED as CLOSE_PROMPT_CLOSED, EXIT_ALWAYS, EXIT_ONCE, HIDE_ALWAYS, HIDE_ONCE,
};
use crate::login::LoginDialog;
use crate::resources::{original_bitmap, original_svg, resources};
use crate::settings::SettingsDialog;
use crate::tray::{AppTray, INIT as TRAY_INIT, OPEN_MENU as TRAY_OPEN_MENU, SHOW_MAIN};

#[derive(Clone, Copy)]
enum ClosePreference {
    HideToTray,
    Exit,
}

#[derive(Default)]
pub(crate) struct MainWindow {
    tray: Option<AppTray>,
    close_preference: Option<ClosePreference>,
    close_prompt_open: bool,
    force_exit: bool,
}

impl WindowImpl for MainWindow {
    fn skin(&self) -> Skin {
        Skin::res("app.xml")
    }

    fn resources(&self) -> flexui::ResourceManager {
        resources()
    }

    fn on_initialized(&mut self, ctx: &mut WindowCtx) {
        // 留到首轮事件泵创建，满足 macOS 状态栏对象必须在运行中的主循环创建的约束。
        if let Some(proxy) = ctx.main_proxy() {
            proxy.send(TRAY_INIT);
        }
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
                open_settings(ctx);
            }
            "minimize" => ctx.minimize(),
            "close_window" => ctx.close(),
            "btn_start" => ctx.set_localized_text("home_status", "app.status.steam_selected"),
            _ => {}
        }
    }

    fn on_message(&mut self, message: &str, ctx: &mut WindowCtx) {
        match message {
            TRAY_INIT => {
                let tooltip = ctx.localized_text("app.tray.tooltip");
                let Some(proxy) = ctx.main_proxy() else {
                    return;
                };
                match AppTray::create(proxy, &tooltip) {
                    Ok(tray) => self.tray = Some(tray),
                    Err(error) => eprintln!("[flexui-examples] {error}"),
                }
            }
            SHOW_MAIN => ctx.show(),
            TRAY_OPEN_MENU => {
                if self.close_prompt_open {
                    return;
                }
                if let Some(command) =
                    ctx.popup_native_menu(&tray_menu(ctx), NativeMenuAnchor::Cursor)
                {
                    self.handle_tray_command(&command, ctx);
                }
            }
            HIDE_ONCE | HIDE_ALWAYS => {
                self.close_prompt_open = false;
                if message == HIDE_ALWAYS {
                    self.close_preference = Some(ClosePreference::HideToTray);
                }
                self.hide_to_tray(ctx);
            }
            EXIT_ONCE | EXIT_ALWAYS => {
                self.close_prompt_open = false;
                if message == EXIT_ALWAYS {
                    self.close_preference = Some(ClosePreference::Exit);
                }
                self.exit(ctx);
            }
            CLOSE_PROMPT_CLOSED => self.close_prompt_open = false,
            _ => {}
        }
    }

    fn on_closing(&mut self, ctx: &mut WindowCtx) -> bool {
        if self.force_exit {
            return true;
        }
        match self.close_preference {
            Some(ClosePreference::HideToTray) => {
                self.hide_to_tray(ctx);
                false
            }
            Some(ClosePreference::Exit) => {
                self.force_exit = true;
                true
            }
            None => {
                self.open_close_prompt(ctx);
                false
            }
        }
    }
}

impl MainWindow {
    fn open_close_prompt(&mut self, ctx: &mut WindowCtx) {
        if self.close_prompt_open {
            return;
        }
        let Some(proxy) = ctx.main_proxy() else {
            return;
        };
        match build_window(ClosePrompt::new(proxy)) {
            Ok(dialog) => {
                self.close_prompt_open = true;
                ctx.open_modal(dialog);
            }
            Err(error) => eprintln!("[flexui-examples] 加载关闭确认窗口失败: {error}"),
        }
    }

    fn hide_to_tray(&self, ctx: &mut WindowCtx) {
        if self.tray.is_some() {
            ctx.hide();
        } else {
            // 托盘初始化失败时不能把唯一恢复入口一并隐藏。
            ctx.minimize();
        }
    }

    fn exit(&mut self, ctx: &mut WindowCtx) {
        self.force_exit = true;
        ctx.quit();
    }

    fn handle_tray_command(&mut self, command: &str, ctx: &mut WindowCtx) {
        match command {
            "tray_show" => ctx.show(),
            "tray_coupon" => {
                ctx.show();
                ctx.set_localized_text("home_status", "app.status.coupon");
            }
            "tray_settings" => {
                ctx.show();
                open_settings(ctx);
            }
            "tray_repair" => {
                ctx.show();
                ctx.set_localized_text("home_status", "app.status.self_repair");
            }
            "tray_exit" => self.exit(ctx),
            _ => {}
        }
    }
}

fn open_settings(ctx: &mut WindowCtx) {
    if let Ok(dialog) = build_window(SettingsDialog) {
        ctx.open_modal(dialog);
    }
}

fn tray_menu(ctx: &WindowCtx) -> NativeMenu {
    let text = |key| ctx.localized_text(key);
    NativeMenu::new()
        .item(NativeMenuItem::new("tray_show", text("app.tray.show_main")))
        .item(NativeMenuItem::new("tray_coupon", text("app.menu.coupon")))
        .item(NativeMenuItem::new(
            "tray_settings",
            text("settings.system"),
        ))
        .item(NativeMenuItem::new(
            "tray_repair",
            text("app.menu.self_repair"),
        ))
        .item(NativeMenuItem::new("tray_exit", text("app.tray.exit")))
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
