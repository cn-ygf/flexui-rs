//! 自定义 NSView 子类：承载 flexui 控件树与事件分发，是 macOS 后端的核心。
//!
//! - `drawRect:`：按窗口尺寸布局控件树，再用 `CgCanvas` 走统一绘制管线。
//! - 鼠标/键盘事件：翻译成 flexui 的 `Event`，交给 `Dispatcher`；若需重绘则 `setNeedsDisplay`。
//! - `isFlipped=true`：左上原点、y 向下，与 flexui 统一坐标系一致。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use flexui_core::{layout_node, paint_tree, Dispatcher, Event, MouseButton, Node, Rect, Size};

use crate::canvas::CgCanvas;

/// 视图内部状态：控件树 + 事件分发器。
pub struct AppState {
    pub root: Node,
    pub disp: Dispatcher,
}

/// 视图 ivars。
pub struct FlexViewIvars {
    state: RefCell<AppState>,
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = FlexViewIvars]
    pub struct FlexView;

    impl FlexView {
        /// 自绘入口：布局 + 统一绘制管线。
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let b = self.bounds();
            let size = Size::new(b.size.width as f32, b.size.height as f32);
            let mut st = self.ivars().state.borrow_mut();
            let AppState { root, disp: _ } = &mut *st;
            let cv_measure = CgCanvas::new();
            // 每帧按窗口尺寸重新布局（小树成本低；也让 TabBox 翻页/缩放即时生效）。
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
            self.dispatch(Event::MouseDown {
                pos: self.point(event),
                button: MouseButton::Left,
            });
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.dispatch(Event::MouseUp {
                pos: self.point(event),
                button: MouseButton::Left,
            });
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
    /// 用给定控件树与分发器创建视图。
    pub fn new(mtm: MainThreadMarker, root: Node, disp: Dispatcher) -> Retained<Self> {
        let ivars = FlexViewIvars {
            state: RefCell::new(AppState { root, disp }),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// 把 NSEvent 的窗口坐标转换为视图（左上原点）逻辑坐标。
    fn point(&self, event: &NSEvent) -> flexui_core::Point {
        let win = event.locationInWindow();
        // convertPoint:fromView:nil 会考虑 isFlipped，得到左上原点坐标。
        let p = self.convertPoint_fromView(win, None);
        flexui_core::Point::new(p.x as f32, p.y as f32)
    }

    /// 分发事件并按需触发重绘。
    fn dispatch(&self, ev: Event) {
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp } = &mut *st;
        disp.handle(root.as_mut(), &ev);
        let need = disp.take_redraw();
        drop(st);
        if need {
            self.setNeedsDisplay(true);
        }
    }

    /// 键盘事件：退格转 KeyDown(8)，其余可见字符转 Char。
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
    }
}
