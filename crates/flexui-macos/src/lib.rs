//! flexui-macos：macOS 平台后端（L1）。
//!
//! 用 AppKit（NSApplication/NSWindow）+ 自定义 NSView 承载 flexui 控件树，
//! 提供 `run` 进入主事件循环。唯一包含 `unsafe`/objc 的层。

mod canvas;
mod view;

pub use canvas::CgCanvas;
pub use view::FlexView;

use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

use flexui_core::{Dispatcher, Node};

/// 窗口配置。
pub struct WindowConfig {
    pub title: String,
    pub width: f32,
    pub height: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "flexui-rs".to_string(),
            width: 640.0,
            height: 440.0,
        }
    }
}

/// 启动应用：创建窗口 + 承载控件树，进入主事件循环（阻塞直到退出）。
///
/// `disp` 由调用方预先构建（可注册 tabbar 绑定等）。
pub fn run(config: WindowConfig, root: Node, disp: Dispatcher) {
    let mtm = MainThreadMarker::new().expect("UI 必须在主线程运行");

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let content = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(config.width as f64, config.height as f64),
    );
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
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
    // 允许接收鼠标移动事件（用于 hover）。
    window.setAcceptsMouseMovedEvents(true);

    let view = FlexView::new(mtm, root, disp);
    window.setContentView(Some(&view));
    // 让视图成为第一响应者以接收键盘事件。
    window.makeFirstResponder(Some(&view));

    window.center();
    window.makeKeyAndOrderFront(None);

    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    println!("[flexui] 窗口已创建，进入事件循环。关闭窗口后用 Cmd-Q 退出。");
    app.run();
}
