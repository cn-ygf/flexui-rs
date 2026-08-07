//! 自定义 NSView 子类：承载 flexui 控件树、事件分发与窗口委托回调。
//!
//! - `drawRect:`：布局 + 统一绘制管线。
//! - 鼠标/键盘：翻译成 `Event` 交 `Dispatcher`；点击具名控件后调窗口委托 `on_activate`。
//! - `isFlipped=true`：左上原点、y 向下。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSView, NSWindow};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

use flexui_core::{
    layout_node, paint_tree, Dispatcher, Event, MouseButton, Node, Rect, Size, WindowCtx,
    WindowDelegate, WindowHandle,
};

use crate::canvas::CgCanvas;

/// macOS 窗口控制句柄（实现平台无关的 WindowHandle）。
pub struct MacWindowHandle {
    window: Retained<NSWindow>,
}

impl WindowHandle for MacWindowHandle {
    fn set_title(&mut self, title: &str) {
        self.window.setTitle(&NSString::from_str(title));
    }
    fn close(&mut self) {
        self.window.close();
    }
    fn minimize(&mut self) {
        self.window.miniaturize(None);
    }
    fn maximize(&mut self) {
        self.window.zoom(None);
    }
    fn restore(&mut self) {
        // 还原：若最小化先取消，再 zoom 回普通尺寸。
        self.window.deminiaturize(None);
        self.window.zoom(None);
    }
}

/// 视图内部状态：控件树 + 分发器 + 窗口委托。
pub struct AppState {
    pub root: Node,
    pub disp: Dispatcher,
    pub delegate: Box<dyn WindowDelegate>,
}

pub struct FlexViewIvars {
    state: RefCell<AppState>,
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = FlexViewIvars]
    pub struct FlexView;

    impl FlexView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let b = self.bounds();
            let size = Size::new(b.size.width as f32, b.size.height as f32);
            let mut st = self.ivars().state.borrow_mut();
            let AppState { root, .. } = &mut *st;
            let cv_measure = CgCanvas::new();
            layout_node(root.as_mut(), Rect::new(0.0, 0.0, size.width, size.height), &cv_measure);
            let mut cv = CgCanvas::new();
            paint_tree(root.as_ref(), &mut cv);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.dispatch(Event::MouseMove { pos: self.point(event) });
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.dispatch(Event::MouseMove { pos: self.point(event) });
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.dispatch(Event::MouseDown { pos: self.point(event), button: MouseButton::Left });
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.dispatch(Event::MouseUp { pos: self.point(event), button: MouseButton::Left });
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.handle_key(event);
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }
    }
);

impl FlexView {
    pub fn new(
        mtm: MainThreadMarker,
        root: Node,
        disp: Dispatcher,
        delegate: Box<dyn WindowDelegate>,
    ) -> Retained<Self> {
        let ivars = FlexViewIvars {
            state: RefCell::new(AppState { root, disp, delegate }),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// 窗口创建后触发 on_init（≈ duilib InitWindow）。
    pub fn fire_init(&self, window: &Retained<NSWindow>) {
        let mut handle = MacWindowHandle { window: window.clone() };
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, delegate, .. } = &mut *st;
        let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
        delegate.on_init(&mut ctx);
        drop(st);
        self.setNeedsDisplay(true);
    }

    fn point(&self, event: &NSEvent) -> flexui_core::Point {
        let win = event.locationInWindow();
        let p = self.convertPoint_fromView(win, None);
        flexui_core::Point::new(p.x as f32, p.y as f32)
    }

    /// 分发事件；处理具名控件激活 → 窗口委托 on_activate；按需重绘。
    fn dispatch(&self, ev: Event) {
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp, delegate } = &mut *st;
        disp.handle(root.as_mut(), &ev);
        let need = disp.take_redraw();
        let acts = disp.take_activations();
        let redraw = need || !acts.is_empty();

        if !acts.is_empty() {
            if let Some(win) = window {
                let mut handle = MacWindowHandle { window: win };
                let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
                for name in &acts {
                    delegate.on_activate(name, &mut ctx);
                }
            }
        }
        drop(st);
        if redraw {
            self.setNeedsDisplay(true);
        }
    }

    fn handle_key(&self, event: &NSEvent) {
        let chars = event.characters();
        if let Some(s) = chars {
            let text = s.to_string();
            for ch in text.chars() {
                let ev = if ch == '\u{7f}' || ch == '\u{8}' {
                    Event::KeyDown { key: 8 }
                } else {
                    Event::Char { ch }
                };
                self.dispatch(ev);
            }
        }
        // 同时把首字符键码抛给窗口委托 on_key。
        if let Some(s) = event.characters() {
            if let Some(ch) = s.to_string().chars().next() {
                let window = self.window();
                let mut st = self.ivars().state.borrow_mut();
                let AppState { root, delegate, .. } = &mut *st;
                if let Some(win) = window {
                    let mut handle = MacWindowHandle { window: win };
                    let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
                    delegate.on_key(ch as u32, &mut ctx);
                }
            }
        }
    }
}
