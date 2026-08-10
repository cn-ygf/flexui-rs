//! 自定义 NSView 子类：承载 flexui 控件树、事件分发与窗口委托回调。
//!
//! - `drawRect:`：布局 + 统一绘制管线。
//! - 鼠标/键盘：翻译成 `Event` 交 `Dispatcher`；点击具名控件后调窗口委托 `on_activate`。
//! - `isFlipped=true`：左上原点、y 向下。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSTextInputClient, NSView, NSWindow, NSWindowDelegate};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound,
    NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString, NSUInteger,
};

use flexui_core::event::keys;
use flexui_core::{
    find_mut_by_id, layout_node, paint_tree, Base, Dispatcher, Event, MouseButton, Node, Rect,
    Size, WindowCtx, WindowDelegate, WindowHandle,
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
            if event.clickCount() >= 2 {
                self.dispatch(Event::DoubleClick { pos: self.point(event) });
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.dispatch(Event::MouseUp { pos: self.point(event), button: MouseButton::Left });
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            self.dispatch(Event::MouseUp { pos: self.point(event), button: MouseButton::Right });
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
            // 交给文本输入系统解释：IME 组合/提交经 NSTextInputClient 回调（insertText/
            // setMarkedText/doCommandBySelector），普通键与功能键同样在其中翻译。
            let arr = NSArray::from_slice(&[event]);
            self.interpretKeyEvents(&arr);
            // 另把首字符键码抛给窗口委托 on_key（与旧行为一致）。
            self.fire_on_key(event);
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

    // NSTextInputClient：接入 macOS 文本输入系统，实现中文等 IME 输入（C:IME）。
    unsafe impl NSTextInputClient for FlexView {
        // 提交文本（普通键 / IME 上屏）：清组合串，逐字符发 Char 事件。
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _replacement: NSRange) {
            let text = ns_to_string(string);
            self.ime_insert(&text);
        }

        // 未映射为可见字符的命令（退格/方向/回车等）→ 平台无关键码。
        #[unsafe(method(doCommandBySelector:))]
        fn do_command(&self, selector: Sel) {
            if let Some(key) = command_to_key(selector.name().to_bytes()) {
                self.dispatch(Event::KeyDown { key });
            }
        }

        // 组合中的预览文本（marked text）：存到焦点控件，绘制时显示带下划线。
        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked(&self, string: &AnyObject, _selected: NSRange, _replacement: NSRange) {
            let text = ns_to_string(string);
            self.ime_set_marked(&text);
        }

        #[unsafe(method(unmarkText))]
        fn unmark(&self) {
            self.ime_clear_marked();
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            let loc = self.with_focused(|b| b.cursor).unwrap_or(0);
            NSRange::new(loc, 0)
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            self.with_focused(|b| {
                if b.marked.is_empty() {
                    NSRange::new(NSNotFound as NSUInteger, 0)
                } else {
                    NSRange::new(b.cursor, b.marked.chars().count())
                }
            })
            .unwrap_or(NSRange::new(NSNotFound as NSUInteger, 0))
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked(&self) -> bool {
            self.with_focused(|b| !b.marked.is_empty()).unwrap_or(false)
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(
            &self,
            _range: NSRange,
            _actual: NSRangePointer,
        ) -> Option<Retained<NSAttributedString>> {
            None
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attrs(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::from_slice(&[])
        }

        // 组合窗口定位：返回焦点控件矩形（屏幕坐标）。
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect(&self, _range: NSRange, _actual: NSRangePointer) -> NSRect {
            let rect = self.with_focused(|b| b.rect);
            let Some(r) = rect else {
                return NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
            };
            let win_rect = self.convertRect_toView(to_nsrect(r), None);
            match self.window() {
                Some(win) => win.convertRectToScreen(win_rect),
                None => win_rect,
            }
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn char_index(&self, _point: NSPoint) -> NSUInteger {
            NSNotFound as NSUInteger
        }
    }
);

/// 把 NSString / NSAttributedString 参数转成 Rust String。
fn ns_to_string(obj: &AnyObject) -> String {
    if let Some(s) = obj.downcast_ref::<NSString>() {
        return s.to_string();
    }
    if let Some(a) = obj.downcast_ref::<NSAttributedString>() {
        return a.string().to_string();
    }
    String::new()
}

/// NSTextInputClient 命令选择子 → 平台无关键码。
fn command_to_key(name: &[u8]) -> Option<u32> {
    Some(match name {
        b"deleteBackward:" => keys::BACKSPACE,
        b"deleteForward:" => keys::DELETE,
        b"moveLeft:" => keys::LEFT,
        b"moveRight:" => keys::RIGHT,
        b"moveUp:" => keys::UP,
        b"moveDown:" => keys::DOWN,
        b"moveToBeginningOfLine:" | b"moveToLeftEndOfLine:" => keys::HOME,
        b"moveToEndOfLine:" | b"moveToRightEndOfLine:" => keys::END,
        b"insertTab:" => keys::TAB,
        b"insertNewline:" | b"insertLineBreak:" => keys::ENTER,
        b"cancelOperation:" => keys::ESCAPE,
        _ => return None,
    })
}

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
        let doubles = disp.take_double_clicks();
        let contexts = disp.take_context_clicks();

        if !acts.is_empty() || !doubles.is_empty() || !contexts.is_empty() {
            if let Some(win) = window {
                let mut handle = MacWindowHandle { window: win };
                let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
                for name in &acts {
                    delegate.on_activate(name, &mut ctx);
                }
                for name in &doubles {
                    delegate.on_double_click(name, &mut ctx);
                }
                for (name, pos) in &contexts {
                    delegate.on_context(name, pos.x, pos.y, &mut ctx);
                }
            }
        }
        drop(st);
        // 整窗重绘优先，否则只失效脏矩形（AppKit 会把绘制裁剪到该区域）。
        if need || !acts.is_empty() || !doubles.is_empty() {
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

    /// 把首字符键码抛给窗口委托 on_key（与 IME 路径并行，供上层监听按键）。
    fn fire_on_key(&self, event: &NSEvent) {
        let Some(s) = event.characters() else { return };
        let Some(ch) = s.to_string().chars().next() else { return };
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, delegate, .. } = &mut *st;
        if let Some(win) = window {
            let mut handle = MacWindowHandle { window: win };
            let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
            delegate.on_key(ch as u32, &mut ctx);
        }
    }

    /// 对当前焦点控件的 Base 执行闭包（IME 读写组合串/光标用）。
    fn with_focused<R>(&self, f: impl FnOnce(&mut Base) -> R) -> Option<R> {
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp, .. } = &mut *st;
        let id = disp.focus()?;
        let w = find_mut_by_id(root.as_mut(), id)?;
        Some(f(w.base_mut()))
    }

    /// IME 提交：清组合串并逐字符发 Char（回车转 ENTER）。
    fn ime_insert(&self, text: &str) {
        self.with_focused(|b| b.marked.clear());
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                self.dispatch(Event::KeyDown { key: keys::ENTER });
            } else if !ch.is_control() {
                self.dispatch(Event::Char { ch });
            }
        }
    }

    /// IME 设置组合串（marked text），只失效焦点控件区域。
    fn ime_set_marked(&self, text: &str) {
        if let Some(r) = self.with_focused(|b| {
            b.marked = text.to_string();
            b.rect
        }) {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// IME 清除组合串。
    fn ime_clear_marked(&self) {
        if let Some(r) = self.with_focused(|b| {
            b.marked.clear();
            b.rect
        }) {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }
}
