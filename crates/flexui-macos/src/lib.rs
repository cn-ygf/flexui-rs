//! flexui-macos：macOS 平台后端（L1）。
//!
//! AppKit（NSApplication/NSWindow）+ 自定义 NSView 承载 flexui 控件树。
//! `run` 由上层 Window 驱动调用，传入控件树、分发器与窗口委托。
//!
//! 仅在 macOS 目标下编译；其它平台为空壳（不引入 objc2/CoreGraphics），
//! 以便 `cargo build --workspace` 在 Linux/Windows 上通过。
#![cfg(target_os = "macos")]

mod canvas;
mod clipboard;
mod dialog;
mod native_menu;
mod view;

pub use canvas::CgCanvas;
pub use clipboard::{get_text as clipboard_get_text, set_text as clipboard_set_text};
pub use dialog::show_dialog;
pub use view::{FlexView, MacWindowHandle};

use objc2::rc::{Retained, Weak};
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSColor, NSImage, NSScreen, NSWindow, NSWindowButton, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSData, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString,
};

use flexui_core::{
    Dispatcher, NewWindow, Node, TitlebarMode, WindowConfig, WindowDelegate, WindowInitialPosition,
    WindowPresentation,
};

use std::sync::Mutex;

/// 设置当前进程的应用图标（Dock、应用切换器）。
pub fn set_application_icon(bytes: &[u8]) {
    let mtm = MainThreadMarker::new().expect("应用图标必须在主线程设置");
    let data = NSData::with_bytes(bytes);
    if let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) {
        let app = NSApplication::sharedApplication(mtm);
        unsafe { app.setApplicationIconImage(Some(&icon)) };
    }
}

/// 启动应用（单窗口）。由 facade 的 `Window` 驱动调用。
pub fn run(config: WindowConfig, root: Node, disp: Dispatcher, delegate: Box<dyn WindowDelegate>) {
    run_multi(vec![NewWindow {
        config,
        root,
        disp,
        delegate,
        presentation: WindowPresentation::Normal,
        localizer: None,
        locale_revision: 0,
    }]);
}

/// 启动应用（多窗口）：一次性建多个窗口，共享同一事件循环。
pub fn run_multi(windows: Vec<NewWindow>) {
    let mtm = MainThreadMarker::new().expect("UI 必须在主线程运行");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    // 保活：AppKit 的 ordered windows 会 retain 窗口，这里再存一份以防万一。
    let mut kept: Vec<Retained<NSWindow>> = Vec::new();
    for spec in windows {
        kept.push(make_window(mtm, spec, None));
    }
    let app_delegate = AppDelegate::new(
        mtm,
        kept.iter().map(|window| Weak::new(&**window)).collect(),
    );
    app.setDelegate(Some(ProtocolObject::from_ref(&*app_delegate)));

    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    println!("[flexui] 窗口已创建，进入事件循环。关闭窗口后用 Cmd-Q 退出。");
    app.run();
    app.setDelegate(None);
    drop(app_delegate);
    drop(kept);
}

struct AppDelegateIvars {
    main_windows: Vec<Weak<NSWindow>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppDelegateIvars]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(
            &self,
            _application: &NSApplication,
            _has_visible_windows: bool,
        ) -> bool {
            let main_window_visible = self
                .ivars()
                .main_windows
                .iter()
                .filter_map(Weak::load)
                .any(|window| window.isVisible());
            if !main_window_visible {
                for weak_window in &self.ivars().main_windows {
                    let Some(window) = weak_window.load() else {
                        continue;
                    };
                    if window.isMiniaturized() {
                        window.deminiaturize(None);
                    }
                    if let Some(view) = window
                        .contentView()
                        .and_then(|view| view.downcast::<FlexView>().ok())
                    {
                        view.resume_timers();
                    }
                    window.makeKeyAndOrderFront(None);
                    break;
                }
            }
            true
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, main_windows: Vec<Weak<NSWindow>>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars { main_windows });
        unsafe { msg_send![super(this), init] }
    }
}

/// 将窗口放到主屏幕可用工作区中央；窗口大于工作区时贴齐左下角。
fn center_window_on_main_screen(mtm: MainThreadMarker, window: &NSWindow) {
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        window.center();
        return;
    };
    let work_area = screen.visibleFrame();
    let frame = window.frame();
    window.setFrameOrigin(centered_window_origin(work_area, frame));
}

/// 让未指定位置的窗口按 AppKit 级联摆放，避免所有窗口重叠或落到 (0, 0)。
fn cascade_window_on_main_screen(mtm: MainThreadMarker, window: &NSWindow) {
    static NEXT_TOP_LEFT: Mutex<Option<(f64, f64)>> = Mutex::new(None);

    let Some(screen) = NSScreen::mainScreen(mtm) else {
        window.center();
        return;
    };
    let work_area = screen.visibleFrame();
    let fallback = (
        work_area.origin.x + 20.0,
        work_area.origin.y + work_area.size.height - 20.0,
    );
    let mut next = NEXT_TOP_LEFT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (x, y) = next.unwrap_or(fallback);
    let following = window.cascadeTopLeftFromPoint(NSPoint::new(x, y));
    *next = Some((following.x, following.y));
}

fn centered_window_origin(work_area: NSRect, frame: NSRect) -> NSPoint {
    NSPoint::new(
        work_area.origin.x + (work_area.size.width - frame.size.width).max(0.0) / 2.0,
        work_area.origin.y + (work_area.size.height - frame.size.height).max(0.0) / 2.0,
    )
}

/// 创建一个原生窗口并接入事件循环（定时器由 run loop 保活）。返回窗口句柄。
pub(crate) fn make_window(
    mtm: MainThreadMarker,
    spec: NewWindow,
    owner: Option<&NSWindow>,
) -> Retained<NSWindow> {
    let NewWindow {
        config,
        root,
        disp,
        delegate,
        presentation,
        localizer,
        locale_revision,
    } = spec;
    let modal_owner = if presentation == WindowPresentation::ModalDialog {
        owner
    } else {
        None
    };
    let content = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(config.width as f64, config.height as f64),
    );
    // 按标题栏模式确定样式。
    let mut style = match config.titlebar {
        // 完全自绘标题栏仍保留透明的原生窗口框架，以获得系统圆角与阴影。
        TitlebarMode::None => {
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::FullSizeContentView
        }
        // 系统 或 隐藏留交通灯：都保留标题/关闭/最小化按钮。
        _ => {
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
        }
    };
    if config.resizable {
        style |= NSWindowStyleMask::Resizable;
    }
    // 隐藏留控件：全尺寸内容视图（内容铺到标题栏区域，交通灯仍在）。
    if config.titlebar == TitlebarMode::HiddenKeepControls {
        style |= NSWindowStyleMask::FullSizeContentView;
    }

    let window: Retained<NSWindow> = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            content,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&config.title));
    window.setAcceptsMouseMovedEvents(true);

    // 与 Windows 分层窗口语义一致：透明仅用于无边框窗口，未绘制区域透出桌面。
    if config.titlebar != TitlebarMode::System && config.transparent {
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
    }

    // 自绘/隐藏模式：标题栏透明且内容铺满窗口。
    if config.titlebar != TitlebarMode::System {
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    }
    window.setHasShadow(config.system_shadow);
    if config.titlebar == TitlebarMode::None {
        for button in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(control) = window.standardWindowButton(button) {
                control.setHidden(true);
            }
        }
    } else if config.titlebar == TitlebarMode::HiddenKeepControls {
        if let Some(control) = window.standardWindowButton(NSWindowButton::ZoomButton) {
            control.setHidden(true);
        }
    }
    // 平台默认策略保持旧行为；精确矩形由 FlexView 的 mouseDown 处理。
    if config.titlebar != TitlebarMode::System
        && config.drag_region == flexui_core::WindowDragRegion::PlatformDefault
    {
        window.setMovableByWindowBackground(true);
    }

    let view = FlexView::new(
        mtm,
        root,
        disp,
        delegate,
        view::FlexViewEnvironment {
            localizer,
            locale_revision,
            localized_title: config.localized_title.clone(),
            drag_region: config.drag_region,
            transparent: config.titlebar != TitlebarMode::System && config.transparent,
            modal_owner: modal_owner.map(Weak::new),
        },
    );
    // 注册文件拖放类型（接收拖入的文件路径 → on_drop_files）。
    view.registerForDraggedTypes(&NSArray::from_slice(&[view::filenames_pboard_type()]));
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&view));
    // 视图兼任窗口委托（处理关闭生命周期）。
    window.setDelegate(Some(ProtocolObject::from_ref(&*view)));

    // 窗口/控件就绪后触发初始化生命周期。
    let close_requested = view.fire_init(&window);

    view.resume_timers();

    if let Some(owner) = modal_owner {
        let owner_frame = owner.frame();
        let frame = window.frame();
        window.setFrameOrigin(NSPoint::new(
            owner_frame.origin.x + (owner_frame.size.width - frame.size.width) / 2.0,
            owner_frame.origin.y + (owner_frame.size.height - frame.size.height) / 2.0,
        ));
        if config.visible && window.sheetParent().is_none() {
            owner.beginSheet_completionHandler(&window, None);
        }
    } else {
        match config.initial_position {
            WindowInitialPosition::PlatformDefault => cascade_window_on_main_screen(mtm, &window),
            WindowInitialPosition::CenterScreen => center_window_on_main_screen(mtm, &window),
        }
        if config.visible {
            window.makeKeyAndOrderFront(None);
        }
    }
    if close_requested {
        view.close_after_callback(&window);
    }
    window
}

#[cfg(test)]
mod window_tests {
    use super::*;

    #[test]
    fn center_uses_visible_work_area() {
        let work_area = NSRect::new(NSPoint::new(80.0, 40.0), NSSize::new(1920.0, 1040.0));
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0));
        assert_eq!(
            centered_window_origin(work_area, frame),
            NSPoint::new(640.0, 260.0)
        );
    }

    #[test]
    fn oversized_window_stays_at_work_area_origin() {
        let work_area = NSRect::new(NSPoint::new(-1600.0, 20.0), NSSize::new(1600.0, 900.0));
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(2000.0, 1000.0));
        assert_eq!(
            centered_window_origin(work_area, frame),
            NSPoint::new(-1600.0, 20.0)
        );
    }
}
