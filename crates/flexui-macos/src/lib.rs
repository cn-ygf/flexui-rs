//! flexui-macos：macOS 平台后端（L1）。
//!
//! AppKit（NSApplication/NSWindow）+ 自定义 NSView 承载 flexui 控件树。
//! `run` 由上层 Window 驱动调用，传入控件树、分发器与窗口委托。

mod canvas;
mod view;

pub use canvas::CgCanvas;
pub use view::{FlexView, MacWindowHandle};

use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

use flexui_core::{Dispatcher, Node, WindowConfig, WindowDelegate};

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
    // 基础样式；仅当允许改变大小时加 Resizable（对应 resizable=false）。
    let mut style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable;
    if config.resizable {
        style |= NSWindowStyleMask::Resizable;
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

    let view = FlexView::new(mtm, root, disp, delegate);
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&view));

    // 窗口/控件就绪后触发 on_init（≈ InitWindow）。
    view.fire_init(&window);

    window.center();
    window.makeKeyAndOrderFront(None);

    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    println!("[flexui] 窗口已创建，进入事件循环。关闭窗口后用 Cmd-Q 退出。");
    app.run();
}
