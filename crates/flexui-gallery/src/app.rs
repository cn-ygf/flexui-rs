use flexui::{
    load_native_menu_res, ControlEvent, ImageSource, MenuEntry, MenuStyle, NativeMenu,
    NativeMenuAnchor, NativeMenuItem, NativeSubmenu, Point, ResourceManager, Skin, Theme,
    ThemeMode, WindowCtx, WindowImpl,
};

use crate::{resources, themes};

pub(crate) struct GalleryWindow;

impl WindowImpl for GalleryWindow {
    fn skin(&self) -> Skin {
        Skin::res("gallery.xml")
    }

    fn resources(&self) -> ResourceManager {
        resources::resources()
    }

    fn on_init(&mut self, ctx: &mut WindowCtx) {
        if ctx
            .theme()
            .is_some_and(|theme| theme.mode == ThemeMode::Dark)
        {
            ctx.set_selected("theme_switch", true);
        }
    }

    fn on_control_event(&mut self, name: &str, event: &ControlEvent, ctx: &mut WindowCtx) {
        if let ("theme_switch", ControlEvent::SelectedChanged(dark)) = (name, event) {
            ctx.set_theme(if *dark { Theme::dark() } else { Theme::light() });
        }
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "apply_bilibili_theme" => {
                ctx.set_selected("theme_switch", false);
                ctx.set_theme(themes::bilibili_theme());
            }
            "restore_default_theme" => {
                ctx.set_selected("theme_switch", false);
                ctx.set_theme(Theme::light());
            }
            "open_drawn_menu" => show_drawn_menu(ctx, name),
            "open_xml_native_menu" => {
                let resources = resources::resources();
                match load_native_menu_res(
                    &resources,
                    "menus/native_menu.xml",
                    &flexui::Context::new(),
                ) {
                    Ok(menu) => show_menu(ctx, &menu, NativeMenuAnchor::Control(name.into())),
                    Err(error) => ctx.set_text("native_menu_result", format!("Error: {error}")),
                }
            }
            "open_rust_native_menu" => show_menu(
                ctx,
                &rust_native_menu(),
                NativeMenuAnchor::Control(name.into()),
            ),
            "open_position_native_menu" => show_menu(
                ctx,
                &rust_native_menu(),
                NativeMenuAnchor::Window(Point::new(520.0, 250.0)),
            ),
            command if command.starts_with("drawn_") => {
                ctx.set_text("native_menu_result", command);
            }
            _ => {}
        }
    }

    fn on_context(&mut self, name: &str, x: f32, y: f32, ctx: &mut WindowCtx) {
        if name == "native_context_target" {
            show_menu(
                ctx,
                &rust_native_menu(),
                NativeMenuAnchor::Window(Point::new(x, y)),
            );
        }
    }
}

fn show_drawn_menu(ctx: &mut WindowCtx, control: &str) {
    let Some(anchor) = ctx.get(control, |widget| widget.base().rect) else {
        return;
    };
    let resources = resources::resources();
    let document_icon = resources
        .read("icons/menu_document.svg")
        .ok()
        .map(ImageSource::svg);
    let share_icon = resources
        .read("icons/menu_share.svg")
        .ok()
        .map(ImageSource::svg);
    let mut create = MenuEntry::item("Create item", "drawn_create");
    if let Some(icon) = document_icon {
        create = create.icon(icon);
    }
    let mut share = MenuEntry::submenu(
        "Share",
        vec![
            MenuEntry::item("Copy link", "drawn_copy_link"),
            MenuEntry::item("Email", "drawn_email"),
        ],
    );
    if let Some(icon) = share_icon {
        share = share.icon(icon);
    }
    let style = ctx
        .theme()
        .map_or_else(MenuStyle::default, |theme| MenuStyle::from_theme(&theme));
    ctx.open_styled_menu_entries(anchor, vec![create, share], style);
}

fn show_menu(ctx: &mut WindowCtx, menu: &NativeMenu, anchor: NativeMenuAnchor) {
    if let Some(command) = ctx.popup_native_menu(menu, anchor) {
        ctx.set_text("native_menu_result", command);
    }
}

fn rust_native_menu() -> NativeMenu {
    let resources = resources::resources();
    let document_icon = resources
        .read("icons/menu_document.svg")
        .ok()
        .map(ImageSource::svg);
    let mut create = NativeMenuItem::new("rust_create", "Create item").shortcut("CmdOrCtrl+N");
    if let Some(icon) = document_icon {
        create = create.icon(icon);
    }
    NativeMenu::new()
        .item(create)
        .item(NativeMenuItem::new("rust_rename", "Rename").shortcut("F2"))
        .separator()
        .submenu(
            NativeSubmenu::new("Move to")
                .item(NativeMenuItem::new("rust_inbox", "Inbox").checked(true))
                .item(NativeMenuItem::new("rust_archive", "Archive"))
                .separator()
                .submenu(
                    NativeSubmenu::new("More")
                        .item(NativeMenuItem::new("rust_later", "Later"))
                        .item(NativeMenuItem::new("rust_disabled", "Disabled").enabled(false)),
                ),
        )
        .separator()
        .item(NativeMenuItem::new("rust_delete", "Delete"))
}
