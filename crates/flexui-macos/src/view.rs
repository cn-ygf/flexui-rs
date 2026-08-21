//! 自定义 NSView 子类：承载 flexui 控件树、事件分发与窗口委托回调。
//!
//! - `drawRect:`：按需布局，并只绘制 AppKit 提供的脏区域。
//! - 鼠标/键盘：翻译成 `Event` 交 `Dispatcher`；点击具名控件后调窗口委托 `on_activate`。
//! - `isFlipped=true`：左上原点、y 向下。

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSCursor, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent,
    NSEventModifierFlags, NSTextInputClient, NSView, NSWindow, NSWindowDelegate,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound,
    NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString, NSTimer,
    NSUInteger,
};

use flexui_core::event::keys;
use flexui_core::{
    apply_localizations, find_mut_by_id, layout_node, paint_tree_in_rect, Dispatcher, Event, Mods,
    MouseButton, NewWindow, Node, Point, Rect, Size, Widget, WindowCtx, WindowDelegate,
    WindowDragRegion, WindowHandle,
};

use crate::canvas::{CgCanvas, ImageCache, SharedImageCache};

fn open_overlay_request(disp: &mut Dispatcher, request: flexui_core::OverlayRequest) {
    if let Some(entries) = request.entries {
        disp.open_styled_menu_entries(
            request.anchor,
            entries,
            request.style.unwrap_or_default(),
            request.selected_name,
        );
    } else {
        disp.open_styled_menu(
            request.anchor,
            request.items,
            request.style,
            request.selected_name,
        );
    }
}

/// 逻辑矩形 → NSRect（视图为 flipped，坐标一致）。
fn to_nsrect(r: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(r.origin.x as f64, r.origin.y as f64),
        NSSize::new(r.size.width as f64, r.size.height as f64),
    )
}

/// AppKit 脏矩形 → 框架逻辑矩形（视图为 flipped，坐标方向一致）。
fn from_nsrect(r: NSRect) -> Rect {
    Rect::new(
        r.origin.x as f32,
        r.origin.y as f32,
        r.size.width.max(0.0) as f32,
        r.size.height.max(0.0) as f32,
    )
}

fn event_mods(event: &NSEvent) -> Mods {
    let flags = event.modifierFlags();
    Mods {
        shift: flags.contains(NSEventModifierFlags::Shift),
        ctrl: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        meta: flags.contains(NSEventModifierFlags::Command),
    }
}

fn mac_event_key(event: &NSEvent) -> u32 {
    match event.keyCode() {
        123 => keys::LEFT,
        124 => keys::RIGHT,
        125 => keys::DOWN,
        126 => keys::UP,
        115 => keys::HOME,
        119 => keys::END,
        117 => keys::DELETE,
        51 => keys::BACKSPACE,
        48 => keys::TAB,
        36 | 76 => keys::ENTER,
        53 => keys::ESCAPE,
        key_code => event
            .charactersIgnoringModifiers()
            .and_then(|value| value.to_string().chars().next())
            .map(|ch| ch as u32)
            .unwrap_or(key_code as u32),
    }
}

/// core 使用 Unicode scalar 索引，AppKit 的 NSRange 使用 UTF-16 code unit。
fn char_index_to_utf16(text: &str, char_index: usize) -> NSUInteger {
    text.chars().take(char_index).map(char::len_utf16).sum()
}

/// macOS 窗口控制句柄（实现平台无关的 WindowHandle）。
pub struct MacWindowHandle {
    window: Retained<NSWindow>,
    close_requested: bool,
}

impl MacWindowHandle {
    fn new(window: Retained<NSWindow>) -> Self {
        Self {
            window,
            close_requested: false,
        }
    }

    fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }
}

impl WindowHandle for MacWindowHandle {
    fn set_title(&mut self, title: &str) {
        self.window.setTitle(&NSString::from_str(title));
    }
    fn show(&mut self) {
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        self.window.makeKeyAndOrderFront(None);
        let app = NSApplication::sharedApplication(self.window.mtm());
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
    fn hide(&mut self) {
        self.window.orderOut(None);
    }
    fn quit(&mut self) {
        let app = NSApplication::sharedApplication(self.window.mtm());
        app.terminate(None);
    }
    fn close(&mut self) {
        // AppKit 关闭 sheet 时会同步回调窗口委托。这里只记录请求，由外层在释放
        // AppState 的 RefCell 借用后执行，避免重入导致进程因 borrow panic 退出。
        self.close_requested = true;
    }
    fn minimize(&mut self) {
        self.window.miniaturize(None);
    }
    fn maximize(&mut self) {
        if !self.window.isZoomed() {
            self.window.zoom(None);
        }
    }
    fn restore(&mut self) {
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        if self.window.isZoomed() {
            self.window.zoom(None);
        }
    }
    fn popup_native_menu(
        &mut self,
        menu: &flexui_core::NativeMenu,
        anchor: flexui_core::NativeMenuPopupAnchor,
    ) -> Option<String> {
        crate::native_menu::popup(&self.window, menu, anchor)
    }
}

/// 视图内部状态：控件树 + 分发器 + 窗口委托。
pub struct AppState {
    pub root: Node,
    pub disp: Dispatcher,
    pub delegate: Box<dyn WindowDelegate>,
    pub localizer: Option<flexui_core::Localizer>,
    pub locale_revision: u64,
    pub localized_title: Option<flexui_core::LocalizedStringResource>,
    /// 回调中动态打开的子窗口保活句柄。
    pub child_windows: Vec<Retained<NSWindow>>,
    /// 重复定时器必须在窗口关闭时失效，避免 run loop 继续强持有 FlexView。
    pub timers: Vec<Retained<NSTimer>>,
    /// 每窗口独立图片缓存，随窗口释放原生 NSImage。
    pub image_cache: SharedImageCache,
    /// 控件几何是否需要在下一次绘制前重新布局；帧动画只更新像素，不触发布局。
    pub layout_dirty: bool,
    /// 内容区精确拖动范围；平台默认策略由 NSWindow 自己处理。
    pub drag_region: WindowDragRegion,
    /// owned modal 的 owner；关闭时恢复 owner 输入。
    pub modal_owner: Option<Weak<NSWindow>>,
    /// 原生窗口状态，用于过滤重复的最小化/最大化/恢复通知。
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    /// 防止原生关闭通知异常重复时多次调用 `on_closed`。
    pub closed: bool,
}

/// 延后执行的原生回调：在释放 AppState 借用后再回调窗口委托，避免重入。
type DeferredNativeCallback = Box<dyn FnOnce(&FlexView)>;

pub struct FlexViewIvars {
    state: RefCell<AppState>,
    /// AppKit 会在 hide/show/minimize 等原生调用中同步回调窗口委托；此队列用于避开
    /// 业务回调持有 AppState 可变借用时的重入。
    deferred_native_callbacks: RefCell<Vec<DeferredNativeCallback>>,
}

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = FlexViewIvars]
    pub struct FlexView;

    impl FlexView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            let b = self.bounds();
            let size = Size::new(b.size.width as f32, b.size.height as f32);
            let scale = self
                .window()
                .map(|window| window.backingScaleFactor() as f32)
                .unwrap_or(1.0);
            let mut st = self.ivars().state.borrow_mut();
            let image_cache = st.image_cache.clone();
            let AppState {
                root,
                disp,
                layout_dirty,
                ..
            } = &mut *st;
            if *layout_dirty {
                let cv_measure = CgCanvas::with_image_cache(scale, image_cache.clone());
                layout_node(
                    root.as_mut(),
                    Rect::new(0.0, 0.0, size.width, size.height),
                    &cv_measure,
                );
                *layout_dirty = false;
            }
            let dirty = from_nsrect(dirty);
            let mut cv = CgCanvas::with_image_cache(scale, image_cache);
            paint_tree_in_rect(root.as_ref(), &mut cv, dirty);
            disp.paint_overlays(&mut cv, size);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let p = self.point(event);
            self.dispatch(Event::MouseMove { pos: p });
            self.update_cursor(p);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let p = self.point(event);
            self.dispatch(Event::MouseMove { pos: p });
            self.update_cursor(p);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let pos = self.point(event);
            let should_drag = {
                let st = self.ivars().state.borrow();
                !st.disp.has_overlays()
                    && matches!(st.drag_region, WindowDragRegion::Rect(rect) if rect.contains(pos))
                    && flexui_core::hit_test(st.root.as_ref(), pos).is_none()
            };
            if should_drag {
                if let Some(window) = self.window() {
                    window.performWindowDragWithEvent(event);
                    return;
                }
            }
            self.dispatch(Event::MouseDown {
                pos,
                button: MouseButton::Left,
                mods: event_mods(event),
            });
            if event.clickCount() >= 2 {
                self.dispatch(Event::DoubleClick { pos });
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
            // 精确滚动（触控板）已是像素增量；鼠标滚轮是行增量，需放大为像素，否则滚动过慢。
            let factor = if event.hasPreciseScrollingDeltas() { 1.0 } else { 40.0 };
            self.dispatch(Event::MouseWheel {
                pos: self.point(event),
                dx: event.scrollingDeltaX() as f32 * factor,
                dy: event.scrollingDeltaY() as f32 * factor,
            });
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            // Cmd 快捷键（复制/剪切/粘贴/全选）：无菜单栏时不会经 interpretKeyEvents
            // 派发为 copy:/selectAll: 等，需在此直接拦截。
            if event.modifierFlags().contains(NSEventModifierFlags::Command) {
                if let Some(s) = event.charactersIgnoringModifiers() {
                    if let Some(ch) = s.to_string().chars().next() {
                        match ch.to_ascii_lowercase() {
                            'a' => {
                                self.ime_select_all();
                                return;
                            }
                            'c' => {
                                self.ime_copy();
                                return;
                            }
                            'x' => {
                                self.ime_cut();
                                return;
                            }
                            'v' => {
                                self.ime_paste();
                                return;
                            }
                            _ => {}
                        }
                    }
                }
            }
            if let Some(value) = event.charactersIgnoringModifiers() {
                if let Some(ch) = value.to_string().chars().next().filter(|ch| !ch.is_control()) {
                    self.dispatch(Event::KeyDown { key: ch as u32, mods: event_mods(event) });
                }
            }
            // 交给文本输入系统解释：IME 组合/提交经 NSTextInputClient 回调（insertText/
            // setMarkedText/doCommandBySelector），普通键与功能键同样在其中翻译。
            let arr = NSArray::from_slice(&[event]);
            self.interpretKeyEvents(&arr);
        }

        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            let mods = event_mods(event);
            let key = mac_event_key(event);
            self.dispatch(Event::KeyUp { key, mods });
        }

        // 光标闪烁定时器回调。
        #[unsafe(method(blinkTimer:))]
        fn blink_timer(&self, _timer: &AnyObject) {
            self.fire_blink();
        }

        // 帧定时器回调（驱动动画）。
        #[unsafe(method(frameTimer:))]
        fn frame_timer(&self, _timer: &AnyObject) {
            self.fire_frame();
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

    // FlexView 同时充当窗口委托，处理关闭生命周期。
    unsafe impl NSObjectProtocol for FlexView {}

    unsafe impl NSWindowDelegate for FlexView {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &AnyObject) -> bool {
            let (allow, close_requested) = self.fire_close();
            if allow || close_requested {
                if let Some(window) = self.window() {
                    if window.sheetParent().is_some() {
                        apply_close_request(&window);
                        return false.into();
                    }
                }
            }
            allow || close_requested
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &objc2_foundation::NSNotification) {
            let (timers, owner) = {
                let mut state = self.ivars().state.borrow_mut();
                state.disp.close_main_proxy();
                if !state.closed {
                    state.closed = true;
                    state.delegate.on_closed();
                }
                let timers = std::mem::take(&mut state.timers);
                let owner = state.modal_owner.as_ref().and_then(Weak::load);
                (timers, owner)
            };
            for timer in timers {
                timer.invalidate();
            }
            if let Some(owner) = owner {
                owner.makeKeyAndOrderFront(None);
            }
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &objc2_foundation::NSNotification) {
            self.run_or_defer_native_callback(|view| {
                view.dispatch(Event::WindowFocusChanged { focused: false });
            });
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &objc2_foundation::NSNotification) {
            self.run_or_defer_native_callback(|view| {
                view.dispatch(Event::WindowFocusChanged { focused: true });
            });
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &objc2_foundation::NSNotification) {
            let bounds = self.bounds();
            let width = bounds.size.width as f32;
            let height = bounds.size.height as f32;
            self.run_or_defer_native_callback(move |view| {
                view.dispatch(Event::WindowResized { width, height });
                view.sync_zoom_state();
            });
        }

        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &objc2_foundation::NSNotification) {
            if let Some(window) = self.window() {
                let origin = window.frame().origin;
                let x = origin.x as f32;
                let y = origin.y as f32;
                self.run_or_defer_native_callback(move |view| {
                    view.fire_window_event(flexui_core::WindowEvent::Moved { x, y });
                });
            }
        }

        #[unsafe(method(windowDidMiniaturize:))]
        fn window_did_miniaturize(&self, _notification: &objc2_foundation::NSNotification) {
            self.run_or_defer_native_callback(|view| {
                let mut state = view.ivars().state.borrow_mut();
                let changed = !state.minimized;
                state.minimized = true;
                drop(state);
                if changed {
                    view.fire_window_event(flexui_core::WindowEvent::Minimized);
                }
            });
        }

        #[unsafe(method(windowDidDeminiaturize:))]
        fn window_did_deminiaturize(&self, _notification: &objc2_foundation::NSNotification) {
            let zoomed = self.window().is_some_and(|window| window.isZoomed());
            self.run_or_defer_native_callback(move |view| {
                let mut state = view.ivars().state.borrow_mut();
                let changed = state.minimized;
                state.minimized = false;
                state.maximized = zoomed;
                drop(state);
                if changed {
                    view.fire_window_event(flexui_core::WindowEvent::Restored);
                }
            });
        }

        #[unsafe(method(windowDidEnterFullScreen:))]
        fn window_did_enter_full_screen(&self, _notification: &objc2_foundation::NSNotification) {
            self.run_or_defer_native_callback(|view| {
                let mut state = view.ivars().state.borrow_mut();
                let changed = !state.maximized;
                state.fullscreen = true;
                state.maximized = true;
                drop(state);
                if changed {
                    view.fire_window_event(flexui_core::WindowEvent::Maximized);
                }
            });
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn window_did_exit_full_screen(&self, _notification: &objc2_foundation::NSNotification) {
            let zoomed = self.window().is_some_and(|window| window.isZoomed());
            self.run_or_defer_native_callback(move |view| {
                let mut state = view.ivars().state.borrow_mut();
                let restored = state.fullscreen && !zoomed;
                state.fullscreen = false;
                state.maximized = zoomed;
                drop(state);
                if restored {
                    view.fire_window_event(flexui_core::WindowEvent::Restored);
                }
            });
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

        // 未映射为可见字符的命令：剪贴板/全选走漏斗，其余（退格/方向/回车/Shift扩选）→ 键码。
        #[unsafe(method(doCommandBySelector:))]
        fn do_command(&self, selector: Sel) {
            let name = selector.name().to_bytes();
            match name {
                b"copy:" => self.ime_copy(),
                b"cut:" => self.ime_cut(),
                b"paste:" => self.ime_paste(),
                b"selectAll:" => self.ime_select_all(),
                _ => {
                    if let Some((key, shift)) = command_to_key(name) {
                        let mods = Mods { shift, ..Default::default() };
                        self.dispatch(Event::KeyDown { key, mods });
                    }
                }
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
            self.with_focused_widget(|w| {
                let state = w.text_input_state()?;
                let (lo, hi) = state.selection.unwrap_or((state.cursor, state.cursor));
                let loc = char_index_to_utf16(&state.text, lo);
                let end = char_index_to_utf16(&state.text, hi);
                Some(NSRange::new(loc, end - loc))
            }).flatten().unwrap_or(NSRange::new(0, 0))
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            self.with_focused_widget(|w| {
                let state = w.text_input_state()?;
                if state.marked.is_empty() {
                    Some(NSRange::new(NSNotFound as NSUInteger, 0))
                } else {
                    Some(NSRange::new(char_index_to_utf16(&state.text, state.cursor), state.marked.encode_utf16().count()))
                }
            })
            .flatten().unwrap_or(NSRange::new(NSNotFound as NSUInteger, 0))
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked(&self) -> bool {
            self.with_focused_widget(|w| w.text_input_state().is_some_and(|s| !s.marked.is_empty())).unwrap_or(false)
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

        // 组合窗口定位：返回排版后的真实插入点矩形（屏幕坐标）。
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect(&self, _range: NSRange, _actual: NSRangePointer) -> NSRect {
            let rect = self.with_focused_widget(|w| w.text_input_rect().unwrap_or(w.base().rect));
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

    // 文件拖放目标：接受拖入的文件，读路径后回调 on_drop_files。
    unsafe impl NSDraggingDestination for FlexView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            NSDragOperation::Copy
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let pb = sender.draggingPasteboard();
            let mut paths: Vec<String> = Vec::new();
            // propertyListForType(NSFilenamesPboardType) 返回 NSArray<NSString>（路径列表）。
            // 泛型 NSArray 不是 DowncastTarget，直接用 msg_send 读取 count/objectAtIndex。
            let plist = pb.propertyListForType(filenames_pboard_type());
            if let Some(obj) = plist {
                let count: usize = unsafe { msg_send![&*obj, count] };
                for i in 0..count {
                    let s: Retained<NSString> = unsafe { msg_send![&*obj, objectAtIndex: i] };
                    paths.push(s.to_string());
                }
            }
            if paths.is_empty() {
                false
            } else {
                self.fire_drop(paths);
                true
            }
        }
    }
);

/// 文件名拖放类型（NSFilenamesPboardType 已弃用但读路径最简单，集中一处加 allow）。
#[allow(deprecated)]
pub(crate) fn filenames_pboard_type() -> &'static objc2_app_kit::NSPasteboardType {
    unsafe { objc2_app_kit::NSFilenamesPboardType }
}

/// 命中测试：光标下最上层控件是否为文本输入（Edit）。用于切换 I-beam 光标。
fn apply_close_request(window: &NSWindow) {
    if let Some(owner) = window.sheetParent() {
        owner.endSheet(window);
        window.orderOut(None);
    }
    window.close();
}

/// 创建 FlexView 时随窗口传入的环境状态。
pub struct FlexViewEnvironment {
    pub localizer: Option<flexui_core::Localizer>,
    pub locale_revision: u64,
    pub localized_title: Option<flexui_core::LocalizedStringResource>,
    pub drag_region: WindowDragRegion,
    pub modal_owner: Option<Weak<NSWindow>>,
}

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

/// NSTextInputClient 命令选择子 → (平台无关键码, 是否 Shift 扩选)。
///
/// `*AndModifySelection:` 系列即按住 Shift 的移动，映射为对应基础键 + shift=true；
/// 词粒度（moveWord*）近似按字符移动处理。
fn command_to_key(name: &[u8]) -> Option<(u32, bool)> {
    Some(match name {
        b"deleteBackward:" => (keys::BACKSPACE, false),
        b"deleteForward:" => (keys::DELETE, false),
        b"moveLeft:" | b"moveWordLeft:" => (keys::LEFT, false),
        b"moveRight:" | b"moveWordRight:" => (keys::RIGHT, false),
        b"moveUp:" => (keys::UP, false),
        b"moveDown:" => (keys::DOWN, false),
        b"moveToBeginningOfLine:" | b"moveToLeftEndOfLine:" => (keys::HOME, false),
        b"moveToEndOfLine:" | b"moveToRightEndOfLine:" => (keys::END, false),
        b"insertTab:" => (keys::TAB, false),
        b"insertNewline:" | b"insertLineBreak:" => (keys::ENTER, false),
        b"cancelOperation:" => (keys::ESCAPE, false),
        // —— Shift 扩选 ——
        b"moveLeftAndModifySelection:" | b"moveWordLeftAndModifySelection:" => (keys::LEFT, true),
        b"moveRightAndModifySelection:" | b"moveWordRightAndModifySelection:" => {
            (keys::RIGHT, true)
        }
        b"moveUpAndModifySelection:" => (keys::UP, true),
        b"moveDownAndModifySelection:" => (keys::DOWN, true),
        b"moveToBeginningOfLineAndModifySelection:" | b"moveToLeftEndOfLineAndModifySelection:" => {
            (keys::HOME, true)
        }
        b"moveToEndOfLineAndModifySelection:" | b"moveToRightEndOfLineAndModifySelection:" => {
            (keys::END, true)
        }
        _ => return None,
    })
}

impl FlexView {
    /// 普通 Zoom 不会触发全屏通知，通过 resize 后的 `isZoomed` 补齐状态事件。
    fn sync_zoom_state(&self) {
        let Some(window) = self.window() else { return };
        let zoomed = window.isZoomed();
        let event = {
            let mut state = self.ivars().state.borrow_mut();
            if state.fullscreen || state.minimized || state.maximized == zoomed {
                None
            } else {
                state.maximized = zoomed;
                Some(if zoomed {
                    flexui_core::WindowEvent::Maximized
                } else {
                    flexui_core::WindowEvent::Restored
                })
            }
        };
        if let Some(event) = event {
            self.fire_window_event(event);
        }
    }

    fn fire_window_event(&self, event: flexui_core::WindowEvent) {
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState {
            root,
            disp,
            delegate,
            layout_dirty,
            ..
        } = &mut *st;
        let mut overlays = Vec::new();
        let mut anims = Vec::new();
        let mut new_windows = Vec::new();
        let mut close_requested = false;
        if let Some(window) = window.as_ref() {
            let mut handle = MacWindowHandle::new(window.clone());
            let mut ctx = WindowCtx::with_proxy_and_localizer(
                root.as_mut(),
                &mut handle,
                disp.proxy(),
                localizer,
            );
            delegate.on_window_event(&event, &mut ctx);
            overlays = ctx.take_overlay_requests();
            anims = ctx.take_anim_requests();
            new_windows = ctx.take_new_windows();
            disp.invalidate(ctx.take_invalidation());
            close_requested = handle.take_close_request();
        }
        let opened = !overlays.is_empty();
        if !close_requested {
            for request in overlays {
                open_overlay_request(disp, request);
            }
            for request in anims {
                disp.animate(
                    root.as_mut(),
                    &request.name,
                    request.prop,
                    request.to,
                    request.dur_secs,
                    request.easing,
                );
            }
        }
        let redraw = disp.take_redraw();
        let layout = disp.take_layout();
        *layout_dirty |= layout;
        let dirty = disp.take_dirty();
        drop(st);
        if close_requested {
            if let Some(window) = window.as_ref() {
                self.request_close(window);
            }
            return;
        }
        self.create_windows(new_windows);
        if redraw || layout || opened {
            self.setNeedsDisplay(true);
        } else if let Some(rect) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(rect));
        }
    }

    /// 将窗口级环境归组，避免构造器随环境字段增加而持续膨胀。
    pub fn new(
        mtm: MainThreadMarker,
        root: Node,
        disp: Dispatcher,
        delegate: Box<dyn WindowDelegate>,
        environment: FlexViewEnvironment,
    ) -> Retained<Self> {
        let ivars = FlexViewIvars {
            state: RefCell::new(AppState {
                root,
                disp,
                delegate,
                localizer: environment.localizer,
                locale_revision: environment.locale_revision,
                localized_title: environment.localized_title,
                child_windows: Vec::new(),
                timers: Vec::new(),
                image_cache: Rc::new(RefCell::new(ImageCache::default())),
                layout_dirty: true,
                drag_region: environment.drag_region,
                modal_owner: environment.modal_owner,
                minimized: false,
                maximized: false,
                fullscreen: false,
                closed: false,
            }),
            deferred_native_callbacks: RefCell::new(Vec::new()),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// 关闭请求 → 窗口委托 on_closing，返回是否允许关闭。
    pub fn fire_close(&self) -> (bool, bool) {
        let window = self.window();
        let Some(win) = window else {
            return (true, false);
        };
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState {
            root,
            disp,
            delegate,
            layout_dirty,
            ..
        } = &mut *st;
        let mut handle = MacWindowHandle::new(win);
        let mut ctx = WindowCtx::with_proxy_and_localizer(
            root.as_mut(),
            &mut handle,
            disp.proxy(),
            localizer,
        );
        let allow = delegate.on_closing(&mut ctx);
        let overlays = ctx.take_overlay_requests();
        let anims = ctx.take_anim_requests();
        let new_windows = ctx.take_new_windows();
        disp.invalidate(ctx.take_invalidation());
        let close_requested = handle.take_close_request();
        if allow || close_requested {
            return (allow, close_requested);
        }

        for request in overlays {
            open_overlay_request(disp, request);
        }
        for request in anims {
            disp.animate(
                root.as_mut(),
                &request.name,
                request.prop,
                request.to,
                request.dur_secs,
                request.easing,
            );
        }
        let redraw = disp.take_redraw();
        let layout = disp.take_layout();
        *layout_dirty |= layout;
        let dirty = disp.take_dirty();
        drop(st);
        self.create_windows(new_windows);
        if redraw || layout {
            self.setNeedsDisplay(true);
        } else if let Some(rect) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(rect));
        }
        (allow, close_requested)
    }

    /// 依次触发初始化前、初始化和初始化完成钩子。
    pub fn fire_init(&self, window: &Retained<NSWindow>) -> bool {
        let mut handle = MacWindowHandle::new(window.clone());
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState {
            root,
            disp,
            delegate,
            ..
        } = &mut *st;
        let mut ctx = WindowCtx::with_proxy_and_localizer(
            root.as_mut(),
            &mut handle,
            disp.proxy(),
            localizer,
        );
        delegate.on_before_init(&mut ctx);
        delegate.on_init(&mut ctx);
        delegate.on_initialized(&mut ctx);
        let overlays = ctx.take_overlay_requests();
        let anims = ctx.take_anim_requests();
        let new_wins = ctx.take_new_windows();
        disp.invalidate(ctx.take_invalidation());
        let close_requested = handle.take_close_request();
        for r in overlays {
            open_overlay_request(disp, r);
        }
        for a in anims {
            disp.animate(root.as_mut(), &a.name, a.prop, a.to, a.dur_secs, a.easing);
        }
        drop(st);
        self.create_windows(new_wins);
        self.setNeedsDisplay(true);
        close_requested
    }

    pub(crate) fn resume_timers(&self) {
        let mut state = self.ivars().state.borrow_mut();
        if !state.timers.is_empty() {
            return;
        }

        let blink = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.53,
                self,
                objc2::sel!(blinkTimer:),
                None,
                true,
            )
        };
        let frame = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.016,
                self,
                objc2::sel!(frameTimer:),
                None,
                true,
            )
        };
        state.timers = vec![blink, frame];
    }

    pub fn close_after_callback(&self, window: &NSWindow) {
        self.request_close(window);
    }

    /// 回调释放 AppState 借用后执行关闭。sheet 不能用 performClose 再进入
    /// windowShouldClose，否则 AppKit 会在关闭委托尚未返回时结束同一个 sheet。
    fn request_close(&self, window: &NSWindow) {
        if window.sheetParent().is_some() {
            let (allow, close_requested) = self.fire_close();
            if allow || close_requested {
                apply_close_request(window);
            }
        } else {
            window.performClose(None);
        }
    }

    /// 创建回调里请求的新窗口（多窗口）。
    fn create_windows(&self, specs: Vec<NewWindow>) {
        if specs.is_empty() {
            return;
        }
        let mtm = self.mtm();
        let mut created = Vec::with_capacity(specs.len());
        for spec in specs {
            let is_sheet = spec.presentation == flexui_core::WindowPresentation::ModalDialog
                && self.window().is_some();
            let window = crate::make_window(mtm, spec, self.window().as_deref());
            if !is_sheet {
                created.push(window);
            }
        }
        self.ivars()
            .state
            .borrow_mut()
            .child_windows
            .extend(created);
    }

    /// 文件拖放 → 窗口委托 on_drop_files。
    fn fire_drop(&self, paths: Vec<String>) {
        let window = self.window();
        let mut close_requested = false;
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState {
            root,
            disp,
            delegate,
            layout_dirty,
            ..
        } = &mut *st;
        if let Some(win) = window.as_ref() {
            let mut handle = MacWindowHandle::new(win.clone());
            let mut ctx = WindowCtx::with_localizer(root.as_mut(), &mut handle, localizer);
            delegate.on_drop_files(&paths, &mut ctx);
            disp.invalidate(ctx.take_invalidation());
            close_requested = handle.take_close_request();
        }
        let redraw = disp.take_redraw();
        let layout = disp.take_layout();
        *layout_dirty |= layout;
        let dirty = disp.take_dirty();
        drop(st);
        if close_requested {
            self.request_close(window.as_ref().unwrap());
            return;
        }
        if redraw || layout {
            self.setNeedsDisplay(true);
        } else if let Some(rect) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(rect));
        }
    }

    /// 帧定时回调：推进动画、派发后台线程消息，并按需重绘。
    fn fire_frame(&self) {
        self.drain_deferred_native_callbacks();
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let localizer_for_ctx = st.localizer.clone();
        let locale_changed = st
            .localizer
            .as_ref()
            .is_some_and(|localizer| localizer.revision() != st.locale_revision);
        if locale_changed {
            let localizer = st.localizer.as_ref().unwrap().clone();
            apply_localizations(&mut st.root, &localizer);
            if let (Some(window), Some(title)) = (window.as_ref(), st.localized_title.clone()) {
                window.setTitle(&NSString::from_str(&localizer.text(title)));
            }
            st.locale_revision = localizer.revision();
            st.layout_dirty = true;
        }
        let AppState {
            root,
            disp,
            delegate,
            layout_dirty,
            ..
        } = &mut *st;
        if window.as_ref().is_some_and(|window| window.isVisible()) {
            disp.tick_anims(root.as_mut(), 0.016);
        }
        let msgs = disp.drain_messages();
        let tasks = disp.drain_ui_tasks();
        let control_events = disp.take_control_events();
        let mut anim_reqs = Vec::new();
        let mut ov_reqs = Vec::new();
        let mut new_wins = Vec::new();
        let mut invalidation = flexui_core::Invalidation::None;
        let mut close_requested = false;
        if !tasks.is_empty() || !msgs.is_empty() || !control_events.is_empty() {
            if let Some(win) = window.as_ref() {
                let mut handle = MacWindowHandle::new(win.clone());
                let mut ctx = WindowCtx::with_proxy_and_localizer(
                    root.as_mut(),
                    &mut handle,
                    disp.proxy(),
                    localizer_for_ctx.clone(),
                );
                for task in tasks {
                    task(&mut ctx);
                }
                for m in &msgs {
                    delegate.on_message(m, &mut ctx);
                }
                for (name, event) in &control_events {
                    delegate.on_control_event(name, event, &mut ctx);
                }
                anim_reqs = ctx.take_anim_requests();
                ov_reqs = ctx.take_overlay_requests();
                new_wins = ctx.take_new_windows();
                invalidation = ctx.take_invalidation();
                close_requested = handle.take_close_request();
            }
        }
        disp.invalidate(invalidation);
        if !close_requested {
            for r in ov_reqs {
                open_overlay_request(disp, r);
            }
            for a in anim_reqs {
                disp.animate(root.as_mut(), &a.name, a.prop, a.to, a.dur_secs, a.easing);
            }
        }
        let layout = disp.take_layout();
        *layout_dirty |= layout;
        let redraw = disp.take_redraw();
        let dirty = disp.take_dirty();
        drop(st);
        if close_requested {
            self.request_close(window.as_ref().unwrap());
            return;
        }
        self.create_windows(new_wins);
        if redraw || layout || locale_changed {
            self.setNeedsDisplay(true);
        } else if let Some(rect) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(rect));
        }
    }

    /// AppKit 窗口委托可能在 WindowHandle 调用栈内同步重入。状态空闲时立即执行，
    /// 否则推迟到下一帧，避免同一个 RefCell 再次可变借用导致进程 panic。
    fn run_or_defer_native_callback(&self, callback: impl FnOnce(&FlexView) + 'static) {
        let state_available = self.ivars().state.try_borrow_mut().is_ok();
        if state_available {
            callback(self);
        } else {
            self.ivars()
                .deferred_native_callbacks
                .borrow_mut()
                .push(Box::new(callback));
        }
    }

    fn drain_deferred_native_callbacks(&self) {
        let callbacks = std::mem::take(&mut *self.ivars().deferred_native_callbacks.borrow_mut());
        for callback in callbacks {
            callback(self);
        }
    }

    fn point(&self, event: &NSEvent) -> flexui_core::Point {
        let win = event.locationInWindow();
        let p = self.convertPoint_fromView(win, None);
        flexui_core::Point::new(p.x as f32, p.y as f32)
    }

    /// 悬停在文本控件上时切换为 I-beam 光标，否则箭头（滚动条上用箭头）。
    fn update_cursor(&self, p: Point) {
        let over = {
            let st = self.ivars().state.borrow();
            flexui_core::point_wants_text_cursor(st.root.as_ref(), p)
        };
        if over {
            NSCursor::IBeamCursor().set();
        } else {
            NSCursor::arrowCursor().set();
        }
    }

    /// 分发事件；处理具名控件激活 → 窗口委托 on_activate；按需（脏区/整窗）重绘。
    fn dispatch(&self, ev: Event) {
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState {
            root,
            disp,
            delegate,
            layout_dirty,
            ..
        } = &mut *st;
        if matches!(ev, Event::WindowResized { .. } | Event::ScaleChanged { .. }) {
            *layout_dirty = true;
        }
        disp.handle(root.as_mut(), &ev);
        let window_event = flexui_core::WindowEvent::from_event(&ev);
        // 滚轮增量（滚动容器已先行处理，这里再回传窗口层做缩放等自定义）。
        let wheel = if let Event::MouseWheel { dx, dy, .. } = &ev {
            Some((*dx, *dy))
        } else {
            None
        };
        let acts = disp.take_activations();
        let doubles = disp.take_double_clicks();
        let contexts = disp.take_context_clicks();
        let control_events = disp.take_control_events();

        let mut reqs = Vec::new();
        let mut anim_reqs = Vec::new();
        let mut new_wins = Vec::new();
        let mut close_requested = false;
        let mut invalidation = flexui_core::Invalidation::None;
        if !acts.is_empty()
            || !doubles.is_empty()
            || !contexts.is_empty()
            || !control_events.is_empty()
            || window_event.is_some()
            || wheel.is_some()
        {
            if let Some(win) = window.as_ref() {
                let mut handle = MacWindowHandle::new(win.clone());
                let mut ctx =
                    WindowCtx::with_localizer(root.as_mut(), &mut handle, localizer.clone());
                for name in &acts {
                    delegate.on_activate(name, &mut ctx);
                }
                for name in &doubles {
                    delegate.on_double_click(name, &mut ctx);
                }
                for (name, pos) in &contexts {
                    delegate.on_context(name, pos.x, pos.y, &mut ctx);
                }
                for (name, event) in &control_events {
                    delegate.on_control_event(name, event, &mut ctx);
                }
                if let Some((dx, dy)) = wheel {
                    delegate.on_wheel(dx, dy, &mut ctx);
                }
                if let Some(event) = &window_event {
                    delegate.on_window_event(event, &mut ctx);
                    match event {
                        flexui_core::WindowEvent::Resized { width, height } => {
                            delegate.on_size(*width, *height, &mut ctx);
                        }
                        flexui_core::WindowEvent::KeyDown { key, .. } => {
                            delegate.on_key(*key, &mut ctx)
                        }
                        _ => {}
                    }
                }
                reqs = ctx.take_overlay_requests();
                anim_reqs = ctx.take_anim_requests();
                new_wins = ctx.take_new_windows();
                invalidation = ctx.take_invalidation();
                close_requested = handle.take_close_request();
            }
        }
        // 委托里请求的上下文菜单 / 动画 → 交分发器。
        let opened = !reqs.is_empty();
        disp.invalidate(invalidation);
        if !close_requested {
            for r in reqs {
                open_overlay_request(disp, r);
            }
            for a in anim_reqs {
                disp.animate(root.as_mut(), &a.name, a.prop, a.to, a.dur_secs, a.easing);
            }
        }
        let layout = disp.take_layout();
        *layout_dirty |= layout;
        let need = disp.take_redraw();
        let dirty = disp.take_dirty();
        drop(st);
        if close_requested {
            self.request_close(window.as_ref().unwrap());
            return;
        }
        self.create_windows(new_wins);
        // 整窗重绘优先，否则只失效脏矩形（AppKit 会把绘制裁剪到该区域）。
        if need || opened {
            self.setNeedsDisplay(true);
        } else if let Some(r) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// 光标闪烁定时回调：切换焦点控件 caret 相位；顺带驱动 Tooltip 延时显示。
    fn fire_blink(&self) {
        if !self.window().is_some_and(|window| window.isVisible()) {
            return;
        }
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp, .. } = &mut *st;
        let rect = disp.blink(root.as_mut());
        disp.tooltip_tick(root.as_mut());
        let redraw = disp.take_redraw(); // tooltip 显隐会置整窗重绘
        drop(st);
        if redraw {
            self.setNeedsDisplay(true);
        } else if let Some(r) = rect {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// 对当前焦点控件的 Base 执行闭包（IME 读写组合串/光标用）。
    fn with_focused_widget<R>(&self, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp, .. } = &mut *st;
        let id = disp.focus()?;
        let w = find_mut_by_id(root.as_mut(), id)?;
        Some(f(w))
    }

    /// IME 直接修改控件状态时，同步延后光标闪烁。
    fn reset_focused_caret_blink(&self) {
        let mut st = self.ivars().state.borrow_mut();
        let AppState { root, disp, .. } = &mut *st;
        disp.reset_caret_blink(root.as_mut());
    }

    /// IME 提交：清组合串并逐字符发 Char（回车转 ENTER）。
    fn ime_insert(&self, text: &str) {
        self.with_focused_widget(|w| {
            w.clear_marked_text();
        });
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                self.dispatch(Event::KeyDown {
                    key: keys::ENTER,
                    mods: Mods::default(),
                });
            } else if !ch.is_control() {
                self.dispatch(Event::Char { ch });
            }
        }
    }

    /// IME 设置组合串（marked text），只失效焦点控件区域。
    fn ime_set_marked(&self, text: &str) {
        if let Some(r) = self.with_focused_widget(|w| {
            w.set_marked_text(text.to_string());
            w.base().rect
        }) {
            self.reset_focused_caret_blink();
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// IME 清除组合串。
    fn ime_clear_marked(&self) {
        if let Some(r) = self.with_focused_widget(|w| {
            w.clear_marked_text();
            w.base().rect
        }) {
            self.reset_focused_caret_blink();
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// 复制：把焦点控件选中文本写入剪贴板。
    fn ime_copy(&self) {
        let text = {
            let mut st = self.ivars().state.borrow_mut();
            let AppState { root, disp, .. } = &mut *st;
            disp.copy_selection(root.as_mut())
        };
        if let Some(t) = text {
            crate::clipboard::set_text(&t);
        }
    }

    /// 剪切：写剪贴板并删除选区、失效其区域。
    fn ime_cut(&self) {
        let (text, dirty) = {
            let mut st = self.ivars().state.borrow_mut();
            let AppState { root, disp, .. } = &mut *st;
            let t = disp.cut_selection(root.as_mut());
            (t, disp.take_dirty())
        };
        if let Some(t) = &text {
            crate::clipboard::set_text(t);
        }
        if let Some(r) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// 粘贴：读剪贴板文本插入到焦点控件。
    fn ime_paste(&self) {
        let Some(text) = crate::clipboard::get_text() else {
            return;
        };
        let dirty = {
            let mut st = self.ivars().state.borrow_mut();
            let AppState { root, disp, .. } = &mut *st;
            disp.paste(root.as_mut(), &text);
            disp.take_dirty()
        };
        if let Some(r) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// 全选焦点控件文本。
    fn ime_select_all(&self) {
        let dirty = {
            let mut st = self.ivars().state.borrow_mut();
            let AppState { root, disp, .. } = &mut *st;
            disp.select_all_focused(root.as_mut());
            disp.take_dirty()
        };
        if let Some(r) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }
}
