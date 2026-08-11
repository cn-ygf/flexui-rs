//! flexui-macos：macOS 平台后端（L1）。
//!
//! AppKit（NSApplication/NSWindow）+ 自定义 NSView 承载 flexui 控件树。
//! `run` 由上层 Window 驱动调用，传入控件树、分发器与窗口委托。

mod canvas;
mod clipboard;
mod dialog;
mod view;

pub use canvas::CgCanvas;
pub use clipboard::{get_text as clipboard_get_text, set_text as clipboard_set_text};
pub use dialog::show_dialog;
pub use view::{FlexView, MacWindowHandle};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSPoint, NSRect, NSSize, NSString, NSTimer};

use flexui_core::{Dispatcher, Node, TitlebarMode, WindowConfig, WindowDelegate};

/// 启动应用：创建窗口 + 承载控件树，进入主事件循环（阻塞）。
///
/// 由 facade 的 `Window` 驱动调用；`delegate` 承载 on_init/on_activate 等窗口钩子。
pub fn run(config: WindowConfig, root: Node, disp: Dispatcher, delegate: Box<dyn WindowDelegate>) {
    let mtm = MainThreadMarker::new().expect("UI 必须在主线程运行");

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let content = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(config.width as f64, config.height as f64),
    );
    // 按标题栏模式确定样式。
    let mut style = match config.titlebar {
        // 无边框：无系统标题栏/交通灯。
        TitlebarMode::None => NSWindowStyleMask::Borderless,
        // 系统 或 隐藏留交通灯：都保留标题/关闭/最小化按钮。
        _ => NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable,
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

    // 隐藏留控件：标题栏透明 + 隐藏标题文字（交通灯保留）。
    if config.titlebar == TitlebarMode::HiddenKeepControls {
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    }
    // 无边框/隐藏模式允许拖动窗口背景移动窗口。
    if config.titlebar != TitlebarMode::System {
        window.setMovableByWindowBackground(true);
    }

    let view = FlexView::new(mtm, root, disp, delegate);
    // 注册文件拖放类型（接收拖入的文件路径 → on_drop_files）。
    view.registerForDraggedTypes(&NSArray::from_slice(&[view::filenames_pboard_type()]));
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&view));
    // 视图兼任窗口委托（处理 on_close）。
    window.setDelegate(Some(ProtocolObject::from_ref(&*view)));

    // 窗口/控件就绪后触发 on_init（≈ InitWindow）。
    view.fire_init(&window);

    // 光标闪烁定时器（0.53s 切换一次）。
    #[allow(non_snake_case)]
    let _blink = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            0.53,
            &view,
            objc2::sel!(blinkTimer:),
            None,
            true,
        )
    };

    // 帧定时器（~60fps 驱动动画；无动画时回调开销极小，仅在有变化时重绘）。
    #[allow(non_snake_case)]
    let _frame = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            0.016,
            &view,
            objc2::sel!(frameTimer:),
            None,
            true,
        )
    };

    window.center();
    window.makeKeyAndOrderFront(None);

    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    println!("[flexui] 窗口已创建，进入事件循环。关闭窗口后用 Cmd-Q 退出。");
    app.run();
}
