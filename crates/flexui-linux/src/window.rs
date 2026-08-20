//! X11 窗口与事件循环（x11rb，纯 Rust 协议实现——原生 X11，非跨平台封装）。
//!
//! 每帧把控件树渲染进 cairo `ImageSurface`（物理像素），再用 `PutImage` 贴到窗口。
//! 事件：X 事件翻译成 `flexui_core::Event` 交 `Dispatcher::handle`；~60fps 帧循环
//! 推进动画、排空后台消息、按需重绘。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cairo::{Format, ImageSurface};
use flexui_core::{
    layout_node, paint_tree_in_rect, Dispatcher, Event, MouseButton, NewWindow, Node, Point, Rect,
    Size, WindowCtx, WindowDelegate, WindowEvent, WindowHandle,
};
use flexui_core::event::Mods;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, ImageFormat, KeyButMask,
    PropMode, Window, WindowClass,
};
use x11rb::protocol::Event as XEvent;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

use crate::canvas::CairoCanvas;

/// 单个窗口的运行时状态。
struct WinState {
    xid: Window,
    gc: u32,
    depth: u8,
    root: Node,
    disp: Dispatcher,
    delegate: Box<dyn WindowDelegate>,
    localizer: Option<flexui_core::Localizer>,
    layout_dirty: bool,
    width: f32,
    height: f32,
    scale: f32,
    surface: Option<ImageSurface>,
    open: bool,
}

/// 后端给 `WindowCtx` 用的窗口句柄：记录标题/关闭等请求，循环稍后落地。
struct LinuxWindowHandle<'a> {
    conn: &'a RustConnection,
    xid: Window,
    close_requested: bool,
}

impl<'a> LinuxWindowHandle<'a> {
    fn new(conn: &'a RustConnection, xid: Window) -> Self {
        Self {
            conn,
            xid,
            close_requested: false,
        }
    }
}

impl WindowHandle for LinuxWindowHandle<'_> {
    fn set_title(&mut self, title: &str) {
        let _ = self.conn.change_property8(
            PropMode::REPLACE,
            self.xid,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        );
        let _ = self.conn.flush();
    }
    fn show(&mut self) {
        let _ = self.conn.map_window(self.xid);
        let _ = self.conn.flush();
    }
    fn hide(&mut self) {
        let _ = self.conn.unmap_window(self.xid);
        let _ = self.conn.flush();
    }
    fn close(&mut self) {
        self.close_requested = true;
    }
    fn minimize(&mut self) {
        // X11 最小化 = 发 WM_CHANGE_STATE(IconicState)。简化：暂用 unmap。
        let _ = self.conn.unmap_window(self.xid);
        let _ = self.conn.flush();
    }
    fn maximize(&mut self) {}
    fn restore(&mut self) {
        let _ = self.conn.map_window(self.xid);
        let _ = self.conn.flush();
    }
}

/// 启动应用（多窗口）：建窗口、进共享事件循环。
pub fn run_multi(windows: Vec<NewWindow>) {
    let (conn, screen_num) = x11rb::connect(None).expect("连接 X server 失败（需 DISPLAY）");
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let depth = screen.root_depth;
    let visual = screen.root_visual;

    // WM_DELETE_WINDOW 协议原子。
    let wm_protocols = conn
        .intern_atom(false, b"WM_PROTOCOLS")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let wm_delete = conn
        .intern_atom(false, b"WM_DELETE_WINDOW")
        .unwrap()
        .reply()
        .unwrap()
        .atom;

    let mut states: HashMap<Window, WinState> = HashMap::new();

    for spec in windows {
        let xid = conn.generate_id().unwrap();
        let gc = conn.generate_id().unwrap();
        let w = spec.config.width.max(1.0) as u16;
        let h = spec.config.height.max(1.0) as u16;
        let aux = CreateWindowAux::new().event_mask(
            EventMask::EXPOSURE
                | EventMask::KEY_PRESS
                | EventMask::KEY_RELEASE
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::POINTER_MOTION
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::FOCUS_CHANGE,
        );
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            xid,
            root,
            0,
            0,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &aux,
        )
        .unwrap();
        conn.create_gc(gc, xid, &CreateGCAux::new()).unwrap();
        // 标题。
        let _ = conn.change_property8(
            PropMode::REPLACE,
            xid,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            spec.config.title.as_bytes(),
        );
        // 参与 WM_DELETE_WINDOW（关闭按钮走 ClientMessage）。
        let _ = conn.change_property32(
            PropMode::REPLACE,
            xid,
            wm_protocols,
            AtomEnum::ATOM,
            &[wm_delete],
        );
        conn.map_window(xid).unwrap();

        let mut st = WinState {
            xid,
            gc,
            depth,
            root: spec.root,
            disp: spec.disp,
            delegate: spec.delegate,
            localizer: spec.localizer,
            layout_dirty: true,
            width: spec.config.width,
            height: spec.config.height,
            scale: 1.0,
            surface: None,
            open: true,
        };
        // 初始化回调。
        run_delegate_init(&conn, &mut st);
        states.insert(xid, st);
    }
    conn.flush().unwrap();

    event_loop(&conn, wm_delete, states);
}

/// 触发 on_before_init / on_init / on_initialized。
fn run_delegate_init(conn: &RustConnection, st: &mut WinState) {
    let mut handle = LinuxWindowHandle::new(conn, st.xid);
    let localizer = st.localizer.clone();
    let mut ctx = WindowCtx::with_proxy_and_localizer(
        st.root.as_mut(),
        &mut handle,
        st.disp.proxy(),
        localizer,
    );
    st.delegate.on_before_init(&mut ctx);
    st.delegate.on_init(&mut ctx);
    st.delegate.on_initialized(&mut ctx);
    let inval = ctx.take_invalidation();
    st.disp.invalidate(inval);
    let _ = st.disp.take_redraw();
    st.layout_dirty |= st.disp.take_layout();
}

/// 共享事件循环：处理 X 事件 + ~60fps 帧节拍。
fn event_loop(conn: &RustConnection, wm_delete: u32, mut states: HashMap<Window, WinState>) {
    let frame = Duration::from_millis(16);
    let mut last_tick = Instant::now();

    // 首帧渲染。
    for st in states.values_mut() {
        render(conn, st);
    }
    let _ = conn.flush();

    loop {
        if states.is_empty() {
            break;
        }
        // 处理已到达的 X 事件（非阻塞）。
        while let Ok(Some(ev)) = conn.poll_for_event() {
            handle_x_event(conn, wm_delete, &mut states, ev);
        }
        // 帧节拍。
        let now = Instant::now();
        if now.duration_since(last_tick) >= frame {
            let dt = now.duration_since(last_tick).as_secs_f32();
            last_tick = now;
            let ids: Vec<Window> = states.keys().copied().collect();
            for id in ids {
                if let Some(st) = states.get_mut(&id) {
                    tick_frame(conn, st, dt);
                }
            }
        }
        // 收掉已关闭窗口。
        states.retain(|_, st| st.open);
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// 处理单个 X 事件。
fn handle_x_event(
    conn: &RustConnection,
    wm_delete: u32,
    states: &mut HashMap<Window, WinState>,
    ev: XEvent,
) {
    match ev {
        XEvent::Expose(e) => {
            if let Some(st) = states.get_mut(&e.window) {
                render(conn, st);
            }
        }
        XEvent::ConfigureNotify(e) => {
            if let Some(st) = states.get_mut(&e.window) {
                let w = e.width as f32;
                let h = e.height as f32;
                if (w - st.width).abs() > 0.5 || (h - st.height).abs() > 0.5 {
                    st.width = w;
                    st.height = h;
                    st.surface = None;
                    st.layout_dirty = true;
                    dispatch(conn, st, Event::WindowResized { width: w, height: h });
                    render(conn, st);
                }
            }
        }
        XEvent::ClientMessage(e) => {
            // WM_DELETE_WINDOW：走 on_closing。
            if e.data.as_data32()[0] == wm_delete {
                if let Some(st) = states.get_mut(&e.window) {
                    if request_close(conn, st) {
                        close_window(conn, st);
                    }
                }
            }
        }
        XEvent::ButtonPress(e) => {
            if let Some(st) = states.get_mut(&e.event) {
                let pos = logical_pos(st, e.event_x, e.event_y);
                let mods = mods_from_state(e.state);
                let down = |button| Event::MouseDown { pos, button, mods };
                match e.detail {
                    1 => dispatch(conn, st, down(MouseButton::Left)),
                    2 => dispatch(conn, st, down(MouseButton::Middle)),
                    3 => dispatch(conn, st, down(MouseButton::Right)),
                    4 => dispatch(conn, st, Event::MouseWheel { pos, dx: 0.0, dy: 40.0 }),
                    5 => dispatch(conn, st, Event::MouseWheel { pos, dx: 0.0, dy: -40.0 }),
                    6 => dispatch(conn, st, Event::MouseWheel { pos, dx: 40.0, dy: 0.0 }),
                    7 => dispatch(conn, st, Event::MouseWheel { pos, dx: -40.0, dy: 0.0 }),
                    _ => {}
                }
            }
        }
        XEvent::ButtonRelease(e) => {
            if let Some(st) = states.get_mut(&e.event) {
                let pos = logical_pos(st, e.event_x, e.event_y);
                let button = match e.detail {
                    1 => Some(MouseButton::Left),
                    2 => Some(MouseButton::Middle),
                    3 => Some(MouseButton::Right),
                    _ => None,
                };
                if let Some(button) = button {
                    dispatch(conn, st, Event::MouseUp { pos, button });
                }
            }
        }
        XEvent::MotionNotify(e) => {
            if let Some(st) = states.get_mut(&e.event) {
                let pos = logical_pos(st, e.event_x, e.event_y);
                dispatch(conn, st, Event::MouseMove { pos });
            }
        }
        XEvent::FocusIn(e) => {
            if let Some(st) = states.get_mut(&e.event) {
                dispatch(conn, st, Event::WindowFocusChanged { focused: true });
            }
        }
        XEvent::FocusOut(e) => {
            if let Some(st) = states.get_mut(&e.event) {
                dispatch(conn, st, Event::WindowFocusChanged { focused: false });
            }
        }
        _ => {}
    }
}

/// 物理事件坐标 → 逻辑点。
fn logical_pos(st: &WinState, x: i16, y: i16) -> Point {
    Point::new(x as f32 / st.scale, y as f32 / st.scale)
}

/// X11 修饰键位 → 框架 Mods。
fn mods_from_state(state: KeyButMask) -> Mods {
    Mods {
        shift: state.contains(KeyButMask::SHIFT),
        ctrl: state.contains(KeyButMask::CONTROL),
        alt: state.contains(KeyButMask::MOD1),
        meta: state.contains(KeyButMask::MOD4),
    }
}

/// 分发一个事件到控件树，并落地委托回调与重绘。
fn dispatch(conn: &RustConnection, st: &mut WinState, ev: Event) {
    st.disp.handle(st.root.as_mut(), &ev);

    let window_event = WindowEvent::from_event(&ev);
    let acts = st.disp.take_activations();
    let doubles = st.disp.take_double_clicks();
    let contexts = st.disp.take_context_clicks();
    let control_events = st.disp.take_control_events();

    let mut close_requested = false;
    if !acts.is_empty()
        || !doubles.is_empty()
        || !contexts.is_empty()
        || !control_events.is_empty()
        || window_event.is_some()
    {
        let mut handle = LinuxWindowHandle::new(conn, st.xid);
        let localizer = st.localizer.clone();
        let mut ctx = WindowCtx::with_proxy_and_localizer(
            st.root.as_mut(),
            &mut handle,
            st.disp.proxy(),
            localizer,
        );
        for name in &acts {
            st.delegate.on_activate(name, &mut ctx);
        }
        for name in &doubles {
            st.delegate.on_double_click(name, &mut ctx);
        }
        for (name, pos) in &contexts {
            st.delegate.on_context(name, pos.x, pos.y, &mut ctx);
        }
        for (name, event) in &control_events {
            st.delegate.on_control_event(name, event, &mut ctx);
        }
        if let Some(event) = &window_event {
            st.delegate.on_window_event(event, &mut ctx);
            match event {
                WindowEvent::Resized { width, height } => {
                    st.delegate.on_size(*width, *height, &mut ctx);
                }
                WindowEvent::KeyDown { key, .. } => st.delegate.on_key(*key, &mut ctx),
                _ => {}
            }
        }
        let inval = ctx.take_invalidation();
        close_requested = handle.close_requested;
        st.disp.invalidate(inval);
    }

    st.layout_dirty |= st.disp.take_layout();
    let need = st.disp.take_redraw();
    let _ = st.disp.take_dirty();
    if close_requested {
        close_window(conn, st);
        return;
    }
    if need || st.layout_dirty {
        render(conn, st);
    }
}

/// 帧节拍：推进动画、排空后台消息/任务，按需重绘。
fn tick_frame(conn: &RustConnection, st: &mut WinState, dt: f32) {
    st.disp.tick_anims(st.root.as_mut(), dt);

    let msgs = st.disp.drain_messages();
    let tasks = st.disp.drain_ui_tasks();
    let control_events = st.disp.take_control_events();
    if !msgs.is_empty() || !tasks.is_empty() || !control_events.is_empty() {
        let mut handle = LinuxWindowHandle::new(conn, st.xid);
        let localizer = st.localizer.clone();
        let mut ctx = WindowCtx::with_proxy_and_localizer(
            st.root.as_mut(),
            &mut handle,
            st.disp.proxy(),
            localizer,
        );
        for task in tasks {
            task(&mut ctx);
        }
        for m in &msgs {
            st.delegate.on_message(m, &mut ctx);
        }
        for (name, event) in &control_events {
            st.delegate.on_control_event(name, event, &mut ctx);
        }
        let inval = ctx.take_invalidation();
        let close_requested = handle.close_requested;
        st.disp.invalidate(inval);
        if close_requested {
            close_window(conn, st);
            return;
        }
    }
    st.layout_dirty |= st.disp.take_layout();
    let need = st.disp.take_redraw();
    let _ = st.disp.take_dirty();
    if need || st.layout_dirty {
        render(conn, st);
    }
}

/// on_closing 询问是否允许关闭。
fn request_close(conn: &RustConnection, st: &mut WinState) -> bool {
    let mut handle = LinuxWindowHandle::new(conn, st.xid);
    let localizer = st.localizer.clone();
    let mut ctx =
        WindowCtx::with_proxy_and_localizer(st.root.as_mut(), &mut handle, st.disp.proxy(), localizer);
    st.delegate.on_closing(&mut ctx)
}

/// 关闭窗口：销毁 X 窗口、回调 on_closed、标记移除。
fn close_window(conn: &RustConnection, st: &mut WinState) {
    let _ = conn.destroy_window(st.xid);
    let _ = conn.flush();
    st.delegate.on_closed();
    st.open = false;
}

/// 渲染控件树到 ImageSurface 并 PutImage 到窗口。
fn render(conn: &RustConnection, st: &mut WinState) {
    let pw = (st.width * st.scale).ceil().max(1.0) as i32;
    let ph = (st.height * st.scale).ceil().max(1.0) as i32;

    if st.surface.is_none() {
        st.surface = ImageSurface::create(Format::ARgb32, pw, ph).ok();
    }
    let Some(surface) = st.surface.clone() else {
        return;
    };

    // 布局（需要时）。
    if st.layout_dirty {
        let cv = CairoCanvas::new(&surface, st.scale);
        layout_node(
            st.root.as_mut(),
            Rect::new(0.0, 0.0, st.width, st.height),
            &cv,
        );
        st.layout_dirty = false;
    }
    // 绘制整树。
    {
        let mut cv = CairoCanvas::new(&surface, st.scale);
        paint_tree_in_rect(
            st.root.as_ref(),
            &mut cv,
            Rect::new(0.0, 0.0, st.width, st.height),
        );
        st.disp.paint_overlays(&mut cv, Size::new(st.width, st.height));
    }

    present(conn, st, &surface, pw, ph);
}

/// 把 ImageSurface 的像素 PutImage 到窗口。
fn present(conn: &RustConnection, st: &WinState, surface: &ImageSurface, pw: i32, ph: i32) {
    let mut surface = surface.clone();
    surface.flush();
    let stride = surface.stride();
    let Ok(data) = surface.data() else {
        return;
    };
    // cairo ARGB32 stride == pw*4；直接整块上传（ZPixmap，深度取窗口深度）。
    // X TrueColor 32/24 小端为 BGRA，与 cairo ARGB32 内存序一致。
    if stride == pw * 4 {
        let _ = conn.put_image(
            ImageFormat::Z_PIXMAP,
            st.xid,
            st.gc,
            pw as u16,
            ph as u16,
            0,
            0,
            0,
            st.depth,
            &data[..],
        );
        let _ = conn.flush();
    }
}

/// 设置应用图标（X11 用 _NET_WM_ICON，后续实现）。
pub fn set_application_icon(_bytes: &[u8]) {}
