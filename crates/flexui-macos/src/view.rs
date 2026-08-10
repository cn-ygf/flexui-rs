//! 自定义 NSView 子类：承载 flexui 控件树、事件分发与窗口委托回调。
//!
//! - `drawRect:`：布局 + 统一绘制管线。
//! - 鼠标/键盘：翻译成 `Event` 交 `Dispatcher`；点击具名控件后调窗口委托 `on_activate`。
//! - `isFlipped=true`：左上原点、y 向下。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSView, NSWindow, NSWindowDelegate};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use flexui_core::event::keys;
use flexui_core::{
    layout_node, paint_tree, Dispatcher, Event, MouseButton, Node, Rect, Size, WindowCtx,
    WindowDelegate, WindowHandle,
};

use crate::canvas::CgCanvas;

/// 逻辑矩形 → NSRect（视图为 flipped，坐标一致）。
fn to_nsrect(r: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(r.origin.x as f64, r.origin.y as f64),
        NSSize::new(r.size.width as f64, r.size.height as f64),
    )
}

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

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            self.dispatch(Event::MouseWheel {
                pos: self.point(event),
                dx: event.deltaX() as f32,
                dy: event.deltaY() as f32,
            });
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.handle_key(event);
        }

        // 光标闪烁定时器回调。
        #[unsafe(method(blinkTimer:))]
        fn blink_timer(&self, _timer: &AnyObject) {
            self.fire_blink();
        }

        // 背板属性（缩放）变化 → 发 ScaleChanged 并重绘。
        #[unsafe(method(viewDidChangeBackingProperties))]
        fn backing_changed(&self) {
            let scale = self
                .window()
                .map(|w| w.backingScaleFactor() as f32)
                .unwrap_or(1.0);
            self.dispatch(Event::ScaleChanged { scale });
            self.setNeedsDisplay(true);
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

    // FlexView 同时充当窗口委托，处理关闭请求（on_close）。
    unsafe impl NSObjectProtocol for FlexView {}

    unsafe impl NSWindowDelegate for FlexView {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &AnyObject) -> bool {
            self.fire_close()
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

    /// 关闭请求 → 窗口委托 on_close，返回是否允许关闭。
    pub fn fire_close(&self) -> bool {
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, delegate, .. } = &mut *st;
        if let Some(win) = window {
            let mut handle = MacWindowHandle { window: win };
            let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
            delegate.on_close(&mut ctx)
        } else {
            true
        }
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

    /// 分发事件；处理具名控件激活 → 窗口委托 on_activate；按需（脏区/整窗）重绘。
    fn dispatch(&self, ev: Event) {
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp, delegate } = &mut *st;
        disp.handle(root.as_mut(), &ev);
        let need = disp.take_redraw();
        let dirty = disp.take_dirty();
        let acts = disp.take_activations();

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
        // 整窗重绘优先，否则只失效脏矩形（AppKit 会把绘制裁剪到该区域）。
        if need || !acts.is_empty() {
            self.setNeedsDisplay(true);
        } else if let Some(r) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// 光标闪烁定时回调：切换焦点控件 caret 相位，只失效其区域。
    fn fire_blink(&self) {
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp, .. } = &mut *st;
        let rect = disp.blink(root.as_mut());
        drop(st);
        if let Some(r) = rect {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    fn handle_key(&self, event: &NSEvent) {
        let chars = event.characters();
        if let Some(s) = chars {
            let text = s.to_string();
            for ch in text.chars() {
                // 把 macOS 功能键字符映射到平台无关键码；可见字符走 Char。
                let ev = match ch as u32 {
                    0x7f | 0x08 => Event::KeyDown { key: keys::BACKSPACE },
                    0x09 => Event::KeyDown { key: keys::TAB },
                    0xF702 => Event::KeyDown { key: keys::LEFT },
                    0xF703 => Event::KeyDown { key: keys::RIGHT },
                    0xF700 => Event::KeyDown { key: keys::UP },
                    0xF701 => Event::KeyDown { key: keys::DOWN },
                    0xF729 => Event::KeyDown { key: keys::HOME },
                    0xF72B => Event::KeyDown { key: keys::END },
                    0xF728 => Event::KeyDown { key: keys::DELETE },
                    _ if !ch.is_control() => Event::Char { ch },
                    _ => continue,
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
