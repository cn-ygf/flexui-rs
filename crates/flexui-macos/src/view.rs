//! 自定义 NSView 子类：承载 flexui 控件树、事件分发与窗口委托回调。
//!
//! - `drawRect:`：布局 + 统一绘制管线。
//! - 鼠标/键盘：翻译成 `Event` 交 `Dispatcher`；点击具名控件后调窗口委托 `on_activate`。
//! - `isFlipped=true`：左上原点、y 向下。

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSCursor, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent,
    NSEventModifierFlags, NSTextInputClient, NSView, NSWindow, NSWindowDelegate,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound,
    NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString, NSUInteger,
};

use flexui_core::event::keys;
use flexui_core::{
    apply_localizations, find_mut_by_id, layout_node, paint_tree, Dispatcher, Event, Mods, MouseButton, NewWindow,
    Node, Point, Rect, Size, Widget, WidgetRole, WindowCtx, WindowDelegate, WindowDragRegion,
    WindowHandle,
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

/// core 使用 Unicode scalar 索引，AppKit 的 NSRange 使用 UTF-16 code unit。
fn char_index_to_utf16(text: &str, char_index: usize) -> NSUInteger {
    text.chars().take(char_index).map(char::len_utf16).sum()
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
        if let Some(owner) = self.window.sheetParent() {
            owner.endSheet(&self.window);
        } else {
            self.window.close();
        }
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
    pub localizer: Option<flexui_core::Localizer>,
    pub locale_revision: u64,
    pub localized_title: Option<flexui_core::LocalizedStringResource>,
    /// 回调中动态打开的子窗口保活句柄。
    pub child_windows: Vec<Retained<NSWindow>>,
    /// 每窗口独立图片缓存，随窗口释放原生 NSImage。
    pub image_cache: SharedImageCache,
    /// 内容区精确拖动范围；平台默认策略由 NSWindow 自己处理。
    pub drag_region: WindowDragRegion,
    /// owned modal 的 owner；关闭时恢复 owner 输入。
    pub modal_owner: Option<Retained<NSWindow>>,
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
            let scale = self
                .window()
                .map(|window| window.backingScaleFactor() as f32)
                .unwrap_or(1.0);
            let mut st = self.ivars().state.borrow_mut();
            let image_cache = st.image_cache.clone();
            let AppState { root, disp, .. } = &mut *st;
            let cv_measure = CgCanvas::with_image_cache(scale, image_cache.clone());
            layout_node(root.as_mut(), Rect::new(0.0, 0.0, size.width, size.height), &cv_measure);
            let mut cv = CgCanvas::with_image_cache(scale, image_cache);
            paint_tree(root.as_ref(), &mut cv);
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
                matches!(st.drag_region, WindowDragRegion::Rect(rect) if rect.contains(pos))
                    && flexui_core::hit_test(st.root.as_ref(), pos).is_none()
            };
            if should_drag {
                if let Some(window) = self.window() {
                    window.performWindowDragWithEvent(event);
                    return;
                }
            }
            self.dispatch(Event::MouseDown { pos, button: MouseButton::Left });
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
            self.dispatch(Event::MouseWheel {
                pos: self.point(event),
                dx: event.scrollingDeltaX() as f32,
                dy: event.scrollingDeltaY() as f32,
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

    // FlexView 同时充当窗口委托，处理关闭请求（on_close）。
    unsafe impl NSObjectProtocol for FlexView {}

    unsafe impl NSWindowDelegate for FlexView {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &AnyObject) -> bool {
            let allow = self.fire_close();
            if allow {
                if let Some(window) = self.window() {
                    if let Some(owner) = window.sheetParent() {
                        owner.endSheet(&window);
                        return false.into();
                    }
                }
            }
            allow
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &objc2_foundation::NSNotification) {
            if let Some(owner) = self.ivars().state.borrow().modal_owner.as_ref() {
                owner.makeKeyAndOrderFront(None);
            }
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
fn point_over_edit(root: &dyn Widget, p: Point) -> bool {
    fn topmost(node: &dyn Widget, p: Point) -> Option<bool> {
        let b = node.base();
        if !b.visible || !b.rect.contains(p) {
            return None;
        }
        for c in b.children.iter().rev() {
            if let Some(r) = topmost(c.as_ref(), p) {
                return Some(r);
            }
        }
        Some(b.role == WidgetRole::Edit && b.enabled)
    }
    topmost(root, p) == Some(true)
}

/// 创建 FlexView 时随窗口传入的环境状态。
pub struct FlexViewEnvironment {
    pub localizer: Option<flexui_core::Localizer>,
    pub locale_revision: u64,
    pub localized_title: Option<flexui_core::LocalizedStringResource>,
    pub drag_region: WindowDragRegion,
    pub modal_owner: Option<Retained<NSWindow>>,
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
                image_cache: Rc::new(RefCell::new(ImageCache::default())),
                drag_region: environment.drag_region,
                modal_owner: environment.modal_owner,
            }),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// 关闭请求 → 窗口委托 on_close，返回是否允许关闭。
    pub fn fire_close(&self) -> bool {
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState { root, delegate, .. } = &mut *st;
        if let Some(win) = window {
            let mut handle = MacWindowHandle { window: win };
            let mut ctx = WindowCtx::with_localizer(root.as_mut(), &mut handle, localizer);
            delegate.on_close(&mut ctx)
        } else {
            true
        }
    }

    /// 窗口创建后触发 on_init（≈ duilib InitWindow）。
    pub fn fire_init(&self, window: &Retained<NSWindow>) {
        let mut handle = MacWindowHandle {
            window: window.clone(),
        };
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState {
            root,
            disp,
            delegate,
            ..
        } = &mut *st;
        let mut ctx = WindowCtx::with_proxy_and_localizer(
            root.as_mut(), &mut handle, disp.proxy(), localizer,
        );
        delegate.on_init(&mut ctx);
        let overlays = ctx.take_overlay_requests();
        let anims = ctx.take_anim_requests();
        let new_wins = ctx.take_new_windows();
        for r in overlays {
            open_overlay_request(disp, r);
        }
        for a in anims {
            disp.animate(root.as_mut(), &a.name, a.prop, a.to, a.dur_secs, a.easing);
        }
        drop(st);
        self.create_windows(new_wins);
        self.setNeedsDisplay(true);
    }

    /// 创建回调里请求的新窗口（多窗口）。
    fn create_windows(&self, specs: Vec<NewWindow>) {
        if specs.is_empty() {
            return;
        }
        let mtm = self.mtm();
        let mut created = Vec::with_capacity(specs.len());
        for spec in specs {
            created.push(crate::make_window(mtm, spec, self.window().as_deref()));
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
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState { root, delegate, .. } = &mut *st;
        if let Some(win) = window {
            let mut handle = MacWindowHandle { window: win };
            let mut ctx = WindowCtx::with_localizer(root.as_mut(), &mut handle, localizer);
            delegate.on_drop_files(&paths, &mut ctx);
        }
        drop(st);
        self.setNeedsDisplay(true);
    }

    /// 帧定时回调：推进动画、派发后台线程消息，并按需重绘。
    fn fire_frame(&self) {
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let localizer_for_ctx = st.localizer.clone();
        let locale_changed = st.localizer.as_ref().is_some_and(|localizer| {
            localizer.revision() != st.locale_revision
        });
        if locale_changed {
            let localizer = st.localizer.as_ref().unwrap().clone();
            apply_localizations(&mut st.root, &localizer);
            if let (Some(window), Some(title)) = (window.as_ref(), st.localized_title.clone()) {
                window.setTitle(&NSString::from_str(&localizer.text(title)));
            }
            st.locale_revision = localizer.revision();
        }
        let AppState {
            root,
            disp,
            delegate,
            ..
        } = &mut *st;
        let changed = disp.tick_anims(root.as_mut(), 0.016);
        let msgs = disp.drain_messages();
        let mut anim_reqs = Vec::new();
        let mut ov_reqs = Vec::new();
        if !msgs.is_empty() {
            if let Some(win) = window {
                let mut handle = MacWindowHandle { window: win };
                let mut ctx = WindowCtx::with_proxy_and_localizer(
                    root.as_mut(), &mut handle, disp.proxy(), localizer_for_ctx.clone(),
                );
                for m in &msgs {
                    delegate.on_message(m, &mut ctx);
                }
                anim_reqs = ctx.take_anim_requests();
                ov_reqs = ctx.take_overlay_requests();
            }
        }
        for r in ov_reqs {
            open_overlay_request(disp, r);
        }
        for a in anim_reqs {
            disp.animate(root.as_mut(), &a.name, a.prop, a.to, a.dur_secs, a.easing);
        }
        let redraw = changed || !msgs.is_empty() || locale_changed;
        drop(st);
        if redraw {
            self.setNeedsDisplay(true);
        }
    }

    fn point(&self, event: &NSEvent) -> flexui_core::Point {
        let win = event.locationInWindow();
        let p = self.convertPoint_fromView(win, None);
        flexui_core::Point::new(p.x as f32, p.y as f32)
    }

    /// 悬停在文本控件上时切换为 I-beam 光标，否则箭头。
    fn update_cursor(&self, p: Point) {
        let over = {
            let st = self.ivars().state.borrow();
            point_over_edit(st.root.as_ref(), p)
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
            ..
        } = &mut *st;
        disp.handle(root.as_mut(), &ev);
        let need = disp.take_redraw();
        let dirty = disp.take_dirty();
        let acts = disp.take_activations();
        let doubles = disp.take_double_clicks();
        let contexts = disp.take_context_clicks();

        let mut reqs = Vec::new();
        let mut anim_reqs = Vec::new();
        let mut new_wins = Vec::new();
        if !acts.is_empty() || !doubles.is_empty() || !contexts.is_empty() {
            if let Some(win) = window {
                let mut handle = MacWindowHandle { window: win };
                let mut ctx = WindowCtx::with_localizer(root.as_mut(), &mut handle, localizer.clone());
                for name in &acts {
                    delegate.on_activate(name, &mut ctx);
                }
                for name in &doubles {
                    delegate.on_double_click(name, &mut ctx);
                }
                for (name, pos) in &contexts {
                    delegate.on_context(name, pos.x, pos.y, &mut ctx);
                }
                reqs = ctx.take_overlay_requests();
                anim_reqs = ctx.take_anim_requests();
                new_wins = ctx.take_new_windows();
            }
        }
        // 委托里请求的上下文菜单 / 动画 → 交分发器。
        let opened = !reqs.is_empty();
        for r in reqs {
            open_overlay_request(disp, r);
        }
        for a in anim_reqs {
            disp.animate(root.as_mut(), &a.name, a.prop, a.to, a.dur_secs, a.easing);
        }
        drop(st);
        self.create_windows(new_wins);
        // 整窗重绘优先，否则只失效脏矩形（AppKit 会把绘制裁剪到该区域）。
        if need || opened || !acts.is_empty() || !doubles.is_empty() {
            self.setNeedsDisplay(true);
        } else if let Some(r) = dirty {
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// 光标闪烁定时回调：切换焦点控件 caret 相位；顺带驱动 Tooltip 延时显示。
    fn fire_blink(&self) {
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

    /// 把首字符键码抛给窗口委托 on_key（与 IME 路径并行，供上层监听按键）。
    fn fire_on_key(&self, event: &NSEvent) {
        let Some(s) = event.characters() else { return };
        let Some(ch) = s.to_string().chars().next() else {
            return;
        };
        let window = self.window();
        let mut st = self.ivars().state.borrow_mut();
        let localizer = st.localizer.clone();
        let AppState { root, delegate, .. } = &mut *st;
        if let Some(win) = window {
            let mut handle = MacWindowHandle { window: win };
            let mut ctx = WindowCtx::with_localizer(root.as_mut(), &mut handle, localizer);
            delegate.on_key(ch as u32, &mut ctx);
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

    /// IME 提交：清组合串并逐字符发 Char（回车转 ENTER）。
    fn ime_insert(&self, text: &str) {
        self.with_focused_widget(|w| { w.clear_marked_text(); });
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
            self.setNeedsDisplayInRect(to_nsrect(r));
        }
    }

    /// IME 清除组合串。
    fn ime_clear_marked(&self) {
        if let Some(r) = self.with_focused_widget(|w| {
            w.clear_marked_text();
            w.base().rect
        }) {
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
