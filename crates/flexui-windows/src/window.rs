//! Win32 窗口管理与消息循环（L1，Windows）。
//!
//! RegisterClassW + CreateWindowExW 建窗，WndProc 分发消息：WM_PAINT 用 GDI+ 自绘，
//! 鼠标/键盘消息翻译成 flexui 的 `Event` 交给 `Dispatcher`。与 macOS 后端对位。

use std::ptr::{null, null_mut};

use flexui_core::event::keys;
use flexui_core::{
    hit_test, layout_node, paint_tree_in_rect, Canvas, Color, Dispatcher, Event, Mods, MouseButton,
    NewWindow, Node, Point, Rect, TitlebarMode, WindowConfig, WindowCtx, WindowDelegate,
    WindowDragRegion, WindowHandle, Widget, WidgetRole,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMNCRP_ENABLED,
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DWMWCP_ROUND,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, ScreenToClient, UpdateWindow, ValidateRect, HDC,
    PAINTSTRUCT,
};
use windows_sys::Win32::Graphics::GdiPlus as gp;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::Ime::{
    ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, ImmSetCompositionWindow, CFS_POINT,
    COMPOSITIONFORM, GCS_COMPSTR, GCS_RESULTSTR,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
use windows_sys::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::canvas::{GdiCanvas, ImageCache};
use crate::gdiplus::{Gdiplus, OffscreenBitmap, UNIT_PIXEL};

/// 旧配置的默认顶部拖动条高度（逻辑像素）。
const DEFAULT_DRAG_STRIP: f32 = 40.0;
/// 还原消息栈退出后再刷新最小化期间发生的内容更新。
const WM_APP_RESTORE_REDRAW: u32 = WM_APP + 2;

/// 窗口内部状态：控件树 + 分发器 + 窗口委托（通过窗口 USERDATA 关联到 HWND）。
struct AppState {
    root: Node,
    disp: Dispatcher,
    delegate: Box<dyn WindowDelegate>,
    image_cache: ImageCache,
    /// 无边框（自绘标题栏）模式：启用自定义拖动/命中。
    frameless: bool,
    /// 固定尺寸窗口仍保留 WS_THICKFRAME 供 DWM 生成阴影，此字段用于禁止实际缩放。
    resizable: bool,
    /// 内容区的窗口拖动范围。
    drag_region: WindowDragRegion,
    /// 窗口是否处于最小化状态。
    minimized: bool,
    /// 最近一次有效客户区尺寸；同尺寸还原时直接复用 DWM 缓存帧。
    client_size: (u16, u16),
    /// 最小化期间内容发生变化，恢复动画结束后需要异步补画。
    redraw_after_restore: bool,
    /// 窗口级持久离屏缓冲；普通局部重绘不再反复分配整窗位图。
    back_buffer: Option<OffscreenBitmap>,
    back_buffer_size: (i32, i32),
    /// 控件几何是否需要在下一帧重新布局；纯 hot 切换可复用现有布局。
    layout_dirty: bool,
}

/// Windows 窗口控制句柄（实现平台无关的 WindowHandle）。
struct WinWindowHandle {
    hwnd: HWND,
}

impl WindowHandle for WinWindowHandle {
    fn set_title(&mut self, title: &str) {
        unsafe { SetWindowTextW(self.hwnd, wide(title).as_ptr()) };
    }
    fn close(&mut self) {
        unsafe { PostMessageW(self.hwnd, WM_CLOSE, 0, 0) };
    }
    fn minimize(&mut self) {
        unsafe { PostMessageW(self.hwnd, WM_SYSCOMMAND, SC_MINIMIZE as usize, 0) };
    }
    fn maximize(&mut self) {
        unsafe { PostMessageW(self.hwnd, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0) };
    }
    fn restore(&mut self) {
        unsafe { PostMessageW(self.hwnd, WM_SYSCOMMAND, SC_RESTORE as usize, 0) };
    }
}

/// UTF-8 → NUL 结尾 UTF-16。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Win32 `MAKEINTRESOURCEW`：把 16 位资源 ID 编码进指针值，API 不会解引用它。
#[allow(clippy::manual_dangling_ptr)]
const fn int_resource(id: u16) -> *const u16 {
    id as usize as *const u16
}

/// 启动应用（单窗口）。由 facade 的 `Window` 驱动调用。
pub fn run(config: WindowConfig, root: Node, disp: Dispatcher, delegate: Box<dyn WindowDelegate>) {
    run_multi(vec![NewWindow {
        config,
        root,
        disp,
        delegate,
    }]);
}

/// 启动应用（多窗口）：一次性建多个窗口，共享同一消息循环。
pub fn run_multi(windows: Vec<NewWindow>) {
    // GDI+ 必须在任何窗口绘制前初始化，并保持到消息循环结束。
    let Some(_gdi) = Gdiplus::startup() else {
        eprintln!("[flexui] GDI+ 初始化失败，无法创建窗口");
        return;
    };

    unsafe {
        // 开启 Per-Monitor V2 DPI 感知（若宿主未嵌入清单，此调用作为运行期兜底）。
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance = GetModuleHandleW(null());
        let class_name = wide("FlexUiWindowClass");
        // 资源 ID 1 是 Windows 约定的主应用图标；未嵌入时 LoadIconW 返回空。
        let app_icon = LoadIconW(hinstance, int_resource(1));
        let wc = WNDCLASSW {
            // 尺寸变化由 WM_SIZE 显式失效；HREDRAW/VREDRAW 会在还原时制造额外整窗重绘。
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: app_icon,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wc);

        for spec in windows {
            create_window(spec);
        }

        // 消息循环（所有窗口共享；最后一个窗口关闭时 PostQuitMessage 退出）。
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 存活窗口计数（最后一个关闭时退出消息循环）。
static WINDOW_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 配置无边框窗口的系统圆角与阴影；旧系统会忽略不支持的属性。
unsafe fn configure_frameless_dwm(hwnd: HWND, corners: bool, shadow: bool) {
    let policy = if corners || shadow {
        DWMNCRP_ENABLED
    } else {
        DWMNCRP_DISABLED
    };
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_NCRENDERING_POLICY as u32,
        &policy as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&policy) as u32,
    );

    let corner = if corners {
        DWMWCP_ROUND
    } else {
        DWMWCP_DONOTROUND
    };
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE as u32,
        &corner as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&corner) as u32,
    );

    // 阴影由 WS_THICKFRAME + DWM 非客户区渲染提供；客户区内不扩展 glass frame，
    // 否则这圈像素会随窗口激活状态在灰色与白色之间切换。
    let margins = MARGINS {
        cxLeftWidth: 0,
        cxRightWidth: 0,
        cyTopHeight: 0,
        cyBottomHeight: 0,
    };
    DwmExtendFrameIntoClientArea(hwnd, &margins);

    // Windows 11 会为 WS_THICKFRAME 额外绘制激活边框；无边框窗口不需要该描边。
    let border_color = DWMWA_COLOR_NONE;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_BORDER_COLOR as u32,
        &border_color as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&border_color) as u32,
    );
}

/// 创建一个原生窗口并接入共享事件循环（窗口类须已注册）。
unsafe fn create_window(spec: NewWindow) -> HWND {
    let NewWindow {
        config,
        root,
        disp,
        delegate,
    } = spec;
    let hinstance = GetModuleHandleW(null());
    let class_name = wide("FlexUiWindowClass");

    let frameless = config.titlebar != TitlebarMode::System;
    let keep_dwm_frame = config.resizable || config.system_corners || config.system_shadow;
    let mut style = if frameless {
        // 保留 WS_CAPTION 的标准顶层窗口语义，让 Shell/DWM 提供最小化、最大化和还原动画；
        // 可见标题栏仍由 WM_NCCALCSIZE 完全移除。
        let mut value = WS_OVERLAPPED | WS_CAPTION | WS_VISIBLE | WS_SYSMENU | WS_MINIMIZEBOX;
        if config.resizable {
            value |= WS_MAXIMIZEBOX;
        }
        if keep_dwm_frame {
            value |= WS_THICKFRAME;
        }
        value
    } else {
        WS_OVERLAPPEDWINDOW | WS_VISIBLE
    };
    if !config.resizable {
        style &= !WS_MAXIMIZEBOX;
        if !frameless {
            style &= !WS_THICKFRAME;
        }
    }

    // `WindowConfig` 使用逻辑像素；Per-Monitor V2 下 CreateWindowExW 接收物理像素。
    // 同时补偿系统标题栏/边框，确保首次布局的客户区仍等于配置尺寸。
    let dpi = GetDpiForSystem().max(96);
    let scale = dpi as f32 / 96.0;
    let mut outer = RECT {
        left: 0,
        top: 0,
        right: (config.width.max(1.0) * scale).round() as i32,
        bottom: (config.height.max(1.0) * scale).round() as i32,
    };
    if !frameless {
        AdjustWindowRectExForDpi(&mut outer, style, 0, 0, dpi);
    }
    let outer_width = (outer.right - outer.left).max(1);
    let outer_height = (outer.bottom - outer.top).max(1);

    let title = wide(&config.title);
    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        style,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        outer_width,
        outer_height,
        null_mut(),
        null_mut(),
        hinstance,
        null(),
    );
    if hwnd.is_null() {
        eprintln!("[flexui] CreateWindowExW 失败");
        return hwnd;
    }

    let mut client_rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client_rect);
    let state = Box::into_raw(Box::new(AppState {
        root,
        disp,
        delegate,
        image_cache: ImageCache::default(),
        frameless,
        resizable: config.resizable,
        drag_region: config.drag_region,
        minimized: false,
        client_size: (
            (client_rect.right - client_rect.left).max(0) as u16,
            (client_rect.bottom - client_rect.top).max(0) as u16,
        ),
        redraw_after_restore: false,
        back_buffer: None,
        back_buffer_size: (0, 0),
        layout_dirty: true,
    }));
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
    WINDOW_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    if frameless {
        configure_frameless_dwm(hwnd, config.system_corners, config.system_shadow);
        // CreateWindowExW 期间 AppState 尚未挂到 HWND，首次 WM_NCCALCSIZE 仍按系统标题栏计算。
        // 状态就绪后强制重算边框，隐藏标题栏但保留 WS_CAPTION 的 Shell/DWM 动画语义。
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    DragAcceptFiles(hwnd, 1);
    ShowWindow(hwnd, SW_SHOW);
    SetTimer(hwnd, 1, 530, None);
    SetTimer(hwnd, 2, 16, None);

    // on_init（可能请求打开新窗口/浮层/动画）。
    let new_wins = {
        let st = &mut *state;
        let mut handle = WinWindowHandle { hwnd };
        let mut ctx = WindowCtx::with_proxy(st.root.as_mut(), &mut handle, st.disp.proxy());
        st.delegate.on_init(&mut ctx);
        let overlays = ctx.take_overlay_requests();
        let anims = ctx.take_anim_requests();
        let nw = ctx.take_new_windows();
        for r in overlays {
            st.disp.open_menu(r.anchor, r.items);
        }
        for a in anims {
            st.disp.animate(
                st.root.as_mut(),
                &a.name,
                a.prop,
                a.to,
                a.dur_secs,
                a.easing,
            );
        }
        InvalidateRect(hwnd, null(), 0);
        nw
    };
    for w in new_wins {
        create_window(w);
    }
    hwnd
}

/// 取窗口关联的 AppState（可能为空）。
unsafe fn app_state(hwnd: HWND) -> *mut AppState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState
}

/// 鼠标消息 lparam（客户区物理像素）→ 逻辑像素坐标（按 DPI 缩放，与布局一致）。
unsafe fn mouse_pos(hwnd: HWND, lparam: LPARAM) -> flexui_core::Point {
    let x = (lparam & 0xFFFF) as u16 as i16 as f32;
    let y = ((lparam >> 16) & 0xFFFF) as u16 as i16 as f32;
    let dpi = GetDpiForWindow(hwnd);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    flexui_core::Point::new(x / scale, y / scale)
}

/// 分发事件；处理具名控件激活 → 窗口委托 on_activate；按需（脏区/整窗）重绘。
unsafe fn dispatch(hwnd: HWND, state: *mut AppState, ev: Event) -> bool {
    if state.is_null() {
        return false;
    }
    let st = &mut *state;
    let visual_only = matches!(ev, Event::MouseMove { .. });
    st.disp.handle(st.root.as_mut(), &ev);
    let need = st.disp.take_redraw();
    let dirty = st.disp.take_dirty();
    let acts = st.disp.take_activations();
    let doubles = st.disp.take_double_clicks();
    let contexts = st.disp.take_context_clicks();

    let (reqs, anim_reqs, new_wins) =
        if !acts.is_empty() || !doubles.is_empty() || !contexts.is_empty() {
            let mut handle = WinWindowHandle { hwnd };
            let root = &mut st.root;
            let delegate = &mut st.delegate;
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
            (
                ctx.take_overlay_requests(),
                ctx.take_anim_requests(),
                ctx.take_new_windows(),
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
    // 委托里请求的上下文菜单 / 动画 → 交分发器。
    let opened = !reqs.is_empty();
    for r in reqs {
        st.disp.open_menu(r.anchor, r.items);
    }
    for a in anim_reqs {
        st.disp.animate(
            st.root.as_mut(),
            &a.name,
            a.prop,
            a.to,
            a.dur_secs,
            a.easing,
        );
    }
    for w in new_wins {
        create_window(w);
    }
    // 最小化期间保留 DWM 的最后一帧，不积累恢复后立即触发的绘制请求。
    if st.minimized {
        st.redraw_after_restore |=
            need || opened || !acts.is_empty() || !doubles.is_empty() || dirty.is_some();
        return false;
    }
    // 整窗重绘优先，否则只失效脏矩形（BeginPaint 的 HDC 会裁剪到更新区域）。
    if need || opened || !acts.is_empty() || !doubles.is_empty() {
        st.layout_dirty = true;
        InvalidateRect(hwnd, null(), 0);
        true
    } else if let Some(r) = dirty {
        st.layout_dirty |= !visual_only;
        let rc = to_physical_rect(hwnd, r);
        InvalidateRect(hwnd, &rc, 0);
        true
    } else {
        false
    }
}

/// 逻辑矩形 → 物理 RECT（按窗口 DPI 缩放）。
unsafe fn to_physical_rect(hwnd: HWND, r: Rect) -> RECT {
    let dpi = GetDpiForWindow(hwnd);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    RECT {
        left: (r.left() * scale) as i32,
        top: (r.top() * scale) as i32,
        right: (r.right() * scale).ceil() as i32,
        bottom: (r.bottom() * scale).ceil() as i32,
    }
}

/// 读取 IME 组合字符串（GCS_COMPSTR 预览 / GCS_RESULTSTR 结果）。
unsafe fn imm_comp_string(hwnd: HWND, gcs: u32) -> Option<String> {
    let himc = ImmGetContext(hwnd);
    if himc.is_null() {
        return None;
    }
    // 先取字节长度，再取内容（UTF-16LE）。
    let bytes = ImmGetCompositionStringW(himc, gcs, null_mut(), 0);
    let result = if bytes > 0 {
        let n = bytes as usize;
        let mut buf = vec![0u8; n];
        ImmGetCompositionStringW(
            himc,
            gcs,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            n as u32,
        );
        let u16buf: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&u16buf))
    } else {
        None
    };
    ImmReleaseContext(hwnd, himc);
    result
}

/// 对焦点控件的 Base 执行 f，返回其逻辑矩形（供失效重绘）。
unsafe fn with_focus_widget(
    state: *mut AppState,
    f: impl FnOnce(&mut dyn Widget),
) -> Option<Rect> {
    if state.is_null() {
        return None;
    }
    let st = &mut *state;
    let id = st.disp.focus()?;
    let w = flexui_core::find_mut_by_id(st.root.as_mut(), id)?;
    let rect = w.base().rect;
    f(w);
    Some(rect)
}

/// 设置焦点控件的 IME 组合串并失效其区域。
unsafe fn set_marked_on_focus(hwnd: HWND, state: *mut AppState, text: &str) {
    let owned = text.to_string();
    if let Some(r) = with_focus_widget(state, move |w| { w.set_marked_text(owned); }) {
        let rc = to_physical_rect(hwnd, r);
        InvalidateRect(hwnd, &rc, 0);
    }
}

/// 清除焦点控件的 IME 组合串并失效其区域。
unsafe fn clear_marked_on_focus(hwnd: HWND, state: *mut AppState) {
    if let Some(r) = with_focus_widget(state, |w| { w.clear_marked_text(); }) {
        let rc = to_physical_rect(hwnd, r);
        InvalidateRect(hwnd, &rc, 0);
    }
}

/// 把 IME 候选/组合窗定位到焦点控件光标附近（物理客户坐标）。
unsafe fn position_ime(hwnd: HWND, state: *mut AppState) {
    if state.is_null() {
        return;
    }
    let st = &mut *state;
    let Some(id) = st.disp.focus() else { return };
    let Some(w) = flexui_core::find_mut_by_id(st.root.as_mut(), id) else {
        return;
    };
    let r = w.text_input_rect().unwrap_or(w.base().rect);
    let dpi = GetDpiForWindow(hwnd);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    let himc = ImmGetContext(hwnd);
    if himc.is_null() {
        return;
    }
    let form = COMPOSITIONFORM {
        dwStyle: CFS_POINT,
        ptCurrentPos: POINT {
            x: (r.left() * scale) as i32,
            y: (r.bottom() * scale) as i32,
        },
        rcArea: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
    };
    ImmSetCompositionWindow(himc, &form);
    ImmReleaseContext(hwnd, himc);
}

/// 失效焦点控件被标脏的区域（剪贴板操作后重绘）。
unsafe fn invalidate_dirty(hwnd: HWND, st: &mut AppState) {
    if let Some(r) = st.disp.take_dirty() {
        st.layout_dirty = true;
        let rc = to_physical_rect(hwnd, r);
        InvalidateRect(hwnd, &rc, 0);
    }
}

/// Ctrl+C：复制焦点控件选中文本到剪贴板（复制不改界面，无需 hwnd 重绘）。
unsafe fn clipboard_copy(_hwnd: HWND, state: *mut AppState) {
    if state.is_null() {
        return;
    }
    let st = &mut *state;
    if let Some(s) = st.disp.copy_selection(st.root.as_mut()) {
        crate::clipboard::set_text(&s);
    }
}

/// Ctrl+X：剪切。
unsafe fn clipboard_cut(hwnd: HWND, state: *mut AppState) {
    if state.is_null() {
        return;
    }
    let st = &mut *state;
    if let Some(s) = st.disp.cut_selection(st.root.as_mut()) {
        crate::clipboard::set_text(&s);
    }
    invalidate_dirty(hwnd, st);
}

/// Ctrl+V：粘贴。
unsafe fn clipboard_paste(hwnd: HWND, state: *mut AppState) {
    if state.is_null() {
        return;
    }
    if let Some(s) = crate::clipboard::get_text() {
        let st = &mut *state;
        st.disp.paste(st.root.as_mut(), &s);
        invalidate_dirty(hwnd, st);
    }
}

/// 文件拖放 → 窗口委托 on_drop_files。
unsafe fn fire_drop(hwnd: HWND, state: *mut AppState, paths: Vec<String>) {
    if state.is_null() {
        return;
    }
    let st = &mut *state;
    let mut handle = WinWindowHandle { hwnd };
    let root = &mut st.root;
    let delegate = &mut st.delegate;
    let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
    delegate.on_drop_files(&paths, &mut ctx);
    st.layout_dirty = true;
    InvalidateRect(hwnd, null(), 0);
}

/// Ctrl+A：全选。
unsafe fn clipboard_select_all(hwnd: HWND, state: *mut AppState) {
    if state.is_null() {
        return;
    }
    let st = &mut *state;
    st.disp.select_all_focused(st.root.as_mut());
    invalidate_dirty(hwnd, st);
}

/// 窗口过程。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let state = app_state(hwnd);
    match msg {
        // 交给默认过程维护激活状态；lParam=-1 仅禁止重绘不可见的非客户区，避免失焦白边。
        WM_NCACTIVATE if !state.is_null() && (*state).frameless => {
            DefWindowProcW(hwnd, msg, wparam, -1)
        }
        // 自绘标题栏占满整个窗口，同时保留 WS_THICKFRAME 供 DWM 生成圆角、阴影和 Snap。
        WM_NCCALCSIZE if !state.is_null() && (*state).frameless => 0,
        WM_SETCURSOR if !state.is_null() && (lparam as u32 & 0xFFFF) == HTCLIENT => {
            let mut p: POINT = std::mem::zeroed();
            if GetCursorPos(&mut p) != 0 && ScreenToClient(hwnd, &mut p) != 0 {
                let dpi = GetDpiForWindow(hwnd);
                let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
                let logical = Point::new(p.x as f32 / scale, p.y as f32 / scale);
                let cursor = if point_over_edit((*state).root.as_ref(), logical) {
                    IDC_IBEAM
                } else {
                    IDC_ARROW
                };
                SetCursor(LoadCursorW(null_mut(), cursor));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !state.is_null() {
                paint_window(hwnd, hdc, ps.rcPaint, &mut *state);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_APP_RESTORE_REDRAW => {
            if !state.is_null() {
                if (*state).minimized {
                    (*state).redraw_after_restore = true;
                } else {
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            let invalidated = dispatch(
                hwnd,
                state,
                Event::MouseMove {
                    pos: mouse_pos(hwnd, lparam),
                },
            );
            // WM_PAINT 优先级低于连续鼠标消息；立即提交已失效的 hot 区域，避免快速跨项时视觉滞后。
            if invalidated {
                UpdateWindow(hwnd);
            }
            0
        }
        WM_LBUTTONDOWN => {
            dispatch(
                hwnd,
                state,
                Event::MouseDown {
                    pos: mouse_pos(hwnd, lparam),
                    button: MouseButton::Left,
                },
            );
            0
        }
        WM_LBUTTONUP => {
            let invalidated = dispatch(
                hwnd,
                state,
                Event::MouseUp {
                    pos: mouse_pos(hwnd, lparam),
                    button: MouseButton::Left,
                },
            );
            // 点击回调可能显示大面积弹层。同步提交本轮更新，避免 WM_PAINT 被后续鼠标消息推迟，
            // 造成用户需要再次点击才看到弹层的错觉。
            if invalidated {
                UpdateWindow(hwnd);
            }
            0
        }
        // 方向/Home/End/Delete 等特殊键（不产生 WM_CHAR）→ 平台无关键码。
        WM_KEYDOWN => {
            // 高位置位表示按下（GetKeyState 返回 i16，负值即按下）。
            let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
            let shift = GetKeyState(VK_SHIFT as i32) < 0;
            let vk = wparam as u32;
            // Ctrl + C/X/V/A → 剪贴板/全选（拦截，避免杂散 WM_CHAR）。
            if ctrl {
                match vk {
                    0x43 => {
                        clipboard_copy(hwnd, state);
                        return 0;
                    }
                    0x58 => {
                        clipboard_cut(hwnd, state);
                        return 0;
                    }
                    0x56 => {
                        clipboard_paste(hwnd, state);
                        return 0;
                    }
                    0x41 => {
                        clipboard_select_all(hwnd, state);
                        return 0;
                    }
                    _ => {}
                }
            }
            let key = match vk {
                0x25 => keys::LEFT,
                0x27 => keys::RIGHT,
                0x26 => keys::UP,
                0x28 => keys::DOWN,
                0x24 => keys::HOME,
                0x23 => keys::END,
                0x2E => keys::DELETE,
                _ => return DefWindowProcW(hwnd, msg, wparam, lparam), // 其余交系统（保留 WM_CHAR）
            };
            let mods = Mods {
                shift,
                ctrl,
                ..Default::default()
            };
            dispatch(hwnd, state, Event::KeyDown { key, mods });
            0
        }
        WM_RBUTTONUP => {
            dispatch(
                hwnd,
                state,
                Event::MouseUp {
                    pos: mouse_pos(hwnd, lparam),
                    button: MouseButton::Right,
                },
            );
            0
        }
        WM_LBUTTONDBLCLK => {
            dispatch(
                hwnd,
                state,
                Event::DoubleClick {
                    pos: mouse_pos(hwnd, lparam),
                },
            );
            0
        }
        WM_CHAR => {
            // 退格→KeyDown(8)、Tab→KeyDown(9) 焦点遍历、回车(13/\r)→ENTER（多行换行），
            // 其余作为 Char。
            let code = wparam as u32;
            let ev = if code == 8 {
                Event::KeyDown {
                    key: 8,
                    mods: Mods::default(),
                }
            } else if code == 9 {
                Event::KeyDown {
                    key: 9,
                    mods: Mods::default(),
                }
            } else if code == 13 {
                Event::KeyDown {
                    key: keys::ENTER,
                    mods: Mods::default(),
                }
            } else if let Some(ch) = char::from_u32(code) {
                Event::Char { ch }
            } else {
                return 0;
            };
            dispatch(hwnd, state, ev);
            0
        }
        // —— IME 组合（内联绘制组合串，与 macOS 对位）——
        WM_IME_STARTCOMPOSITION => {
            // 抑制系统默认组合窗，改为内联显示；候选窗定位到光标处。
            position_ime(hwnd, state);
            0
        }
        WM_IME_COMPOSITION => {
            let flags = lparam as u32;
            // 上屏结果串 → 逐字符发 Char 并清组合串。
            if flags & GCS_RESULTSTR != 0 {
                if let Some(s) = imm_comp_string(hwnd, GCS_RESULTSTR) {
                    clear_marked_on_focus(hwnd, state);
                    for ch in s.chars() {
                        if !ch.is_control() {
                            dispatch(hwnd, state, Event::Char { ch });
                        }
                    }
                }
            }
            // 组合中的预览串 → 存到焦点控件、重定位候选窗。
            if flags & GCS_COMPSTR != 0 {
                let comp = imm_comp_string(hwnd, GCS_COMPSTR).unwrap_or_default();
                set_marked_on_focus(hwnd, state, &comp);
                position_ime(hwnd, state);
            }
            0
        }
        WM_IME_ENDCOMPOSITION => {
            clear_marked_on_focus(hwnd, state);
            0
        }
        // 文件拖放：读所有路径 → on_drop_files。
        WM_DROPFILES => {
            let hdrop = wparam as HDROP;
            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, null_mut(), 0);
            let mut paths: Vec<String> = Vec::with_capacity(count as usize);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, null_mut(), 0);
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u16; len as usize + 1];
                let n = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32);
                paths.push(String::from_utf16_lossy(&buf[..n as usize]));
            }
            DragFinish(hdrop);
            if !paths.is_empty() {
                fire_drop(hwnd, state, paths);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16;
            // 一格(120) ≈ 4px 逻辑；lparam 为屏幕坐标 → 客户区 → 逻辑坐标。
            let dy = delta as f32 / 30.0;
            let mut pt = POINT {
                x: (lparam & 0xFFFF) as u16 as i16 as i32,
                y: ((lparam >> 16) & 0xFFFF) as u16 as i16 as i32,
            };
            ScreenToClient(hwnd, &mut pt);
            let dpi = GetDpiForWindow(hwnd);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let pos = flexui_core::Point::new(pt.x as f32 / scale, pt.y as f32 / scale);
            dispatch(hwnd, state, Event::MouseWheel { pos, dx: 0.0, dy });
            0
        }
        WM_SIZE => {
            if wparam == SIZE_MINIMIZED as usize {
                if !state.is_null() {
                    (*state).minimized = true;
                }
                // 最小化会携带 0 x 0 客户区，不能用它破坏当前布局。
                // 丢弃系统在最小化过程中生成的无效区，恢复时直接复用稳定帧。
                ValidateRect(hwnd, null());
                return 0;
            }
            if wparam != SIZE_RESTORED as usize && wparam != SIZE_MAXIMIZED as usize {
                // WS_POPUP 还可能收到其他窗口触发的 SIZE_MAXSHOW / SIZE_MAXHIDE 广播。
                return 0;
            }

            let w = (lparam & 0xFFFF) as u16;
            let h = ((lparam >> 16) & 0xFFFF) as u16;
            let (restored_from_minimized, size_changed, redraw_after_restore) = if state.is_null() {
                (false, true, false)
            } else {
                let st = &mut *state;
                let restored = st.minimized;
                st.minimized = false;
                (
                    restored,
                    std::mem::replace(&mut st.client_size, (w, h)) != (w, h),
                    std::mem::take(&mut st.redraw_after_restore),
                )
            };
            if !size_changed && !redraw_after_restore {
                // 同尺寸还原无需产生新帧，DWM 可直接复用最小化前的重定向表面。
                return 0;
            }
            if size_changed {
                if !state.is_null() {
                    (*state).layout_dirty = true;
                }
                dispatch(
                    hwnd,
                    state,
                    Event::WindowResized {
                        width: w as f32,
                        height: h as f32,
                    },
                );
            }
            if restored_from_minimized {
                PostMessageW(hwnd, WM_APP_RESTORE_REDRAW, 0, 0);
            } else {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        // 定时器：id=1 光标闪烁 + Tooltip 延时；id=2 帧定时器驱动动画。
        WM_TIMER => {
            if !state.is_null() {
                let st = &mut *state;
                if wparam == 2 {
                    let changed = if st.minimized {
                        false
                    } else {
                        st.disp.tick_anims(st.root.as_mut(), 0.016)
                    };
                    let msgs = st.disp.drain_messages();
                    let mut anim_reqs = Vec::new();
                    let mut ov_reqs = Vec::new();
                    if !msgs.is_empty() {
                        let mut handle = WinWindowHandle { hwnd };
                        let mut ctx =
                            WindowCtx::with_proxy(st.root.as_mut(), &mut handle, st.disp.proxy());
                        for m in &msgs {
                            st.delegate.on_message(m, &mut ctx);
                        }
                        anim_reqs = ctx.take_anim_requests();
                        ov_reqs = ctx.take_overlay_requests();
                    }
                    for r in ov_reqs {
                        st.disp.open_menu(r.anchor, r.items);
                    }
                    for a in anim_reqs {
                        st.disp.animate(
                            st.root.as_mut(),
                            &a.name,
                            a.prop,
                            a.to,
                            a.dur_secs,
                            a.easing,
                        );
                    }
                    if changed || !msgs.is_empty() {
                        if st.minimized {
                            st.redraw_after_restore = true;
                        } else {
                            st.layout_dirty = true;
                            InvalidateRect(hwnd, null(), 0);
                        }
                    }
                } else if !st.minimized {
                    let blink = st.disp.blink(st.root.as_mut());
                    st.disp.tooltip_tick(st.root.as_mut());
                    if st.disp.take_redraw() {
                        st.layout_dirty = true;
                        InvalidateRect(hwnd, null(), 0);
                    } else if let Some(r) = blink {
                        let rc = to_physical_rect(hwnd, r);
                        InvalidateRect(hwnd, &rc, 0);
                    }
                }
            }
            0
        }
        // 背景擦除由我们的双缓冲全覆盖，拦截以消除闪烁。
        WM_ERASEBKGND => 1,
        WM_SYSCOMMAND
            if !state.is_null() && !(*state).resizable && (wparam & 0xFFF0) == SC_SIZE as usize =>
        {
            0
        }
        // 关闭请求 → 窗口委托 on_close；返回 false 阻止关闭。
        WM_CLOSE => {
            let allow = if state.is_null() {
                true
            } else {
                let st = &mut *state;
                let mut handle = WinWindowHandle { hwnd };
                let root = &mut st.root;
                let delegate = &mut st.delegate;
                let mut ctx = WindowCtx::new(root.as_mut(), &mut handle);
                delegate.on_close(&mut ctx)
            };
            if allow {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            } else {
                0
            }
        }
        // 无边框：顶部空白条可拖动（有实体控件处放行给控件），实现自绘标题栏拖动 + Aero Snap。
        WM_NCHITTEST if !state.is_null() && (*state).frameless => {
            let default = DefWindowProcW(hwnd, msg, wparam, lparam);
            if default != HTCLIENT as isize {
                let resize_edge = matches!(
                    default as u32,
                    HTLEFT
                        | HTRIGHT
                        | HTTOP
                        | HTBOTTOM
                        | HTTOPLEFT
                        | HTTOPRIGHT
                        | HTBOTTOMLEFT
                        | HTBOTTOMRIGHT
                );
                if !(*state).resizable && resize_edge {
                    return HTBORDER as isize;
                }
                return default; // 边缘缩放命中交给系统（保留 Snap/缩放）
            }
            // 客户区内：换算成逻辑坐标，判断是否在顶部拖动条且非实体控件之上。
            let mut pt = POINT {
                x: (lparam & 0xFFFF) as u16 as i16 as i32,
                y: ((lparam >> 16) & 0xFFFF) as u16 as i16 as i32,
            };
            ScreenToClient(hwnd, &mut pt);
            let dpi = GetDpiForWindow(hwnd);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let lp = Point::new(pt.x as f32 / scale, pt.y as f32 / scale);
            let st = &*state;
            let in_drag_region = match st.drag_region {
                WindowDragRegion::PlatformDefault => lp.y < DEFAULT_DRAG_STRIP,
                WindowDragRegion::Disabled => false,
                WindowDragRegion::Rect(rect) => rect.contains(lp),
            };
            if in_drag_region && hit_test(st.root.as_ref(), lp).is_none() {
                HTCAPTION as isize
            } else {
                HTCLIENT as isize
            }
        }
        // 跨不同 DPI 显示器移动：按系统建议的新矩形调整窗口，再重绘。
        WM_DPICHANGED => {
            let prc = lparam as *const RECT;
            if !prc.is_null() {
                let r = &*prc;
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            // 缩放变化事件（新 DPI 在 wparam 低字）。
            let dpi = (wparam & 0xFFFF) as u32;
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            dispatch(hwnd, state, Event::ScaleChanged { scale });
            if !state.is_null() {
                (*state).layout_dirty = true;
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_DESTROY => {
            if !state.is_null() {
                drop(Box::from_raw(state));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            // 仅在最后一个窗口关闭时退出消息循环（多窗口）。
            if WINDOW_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) <= 1 {
                PostQuitMessage(0);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 命中最上层可交互控件，判断当前位置是否应显示文本输入光标。
fn point_over_edit(root: &dyn Widget, point: Point) -> bool {
    let Some(hit) = hit_test(root, point) else {
        return false;
    };
    fn is_enabled_edit(node: &dyn Widget, id: flexui_core::WidgetId) -> Option<bool> {
        if node.base().id == id {
            return Some(node.base().role == WidgetRole::Edit && node.base().enabled);
        }
        node.base().children.iter().find_map(|child| is_enabled_edit(child.as_ref(), id))
    }
    is_enabled_edit(root, hit) == Some(true)
}

/// WM_PAINT 期间：双缓冲 + DPI 缩放绘制整棵控件树。
///
/// 先画到 32bpp 离屏位图（抗锯齿/ClearType 在位图上效果最佳），再整块 blit 到窗口，
/// 既消除 resize 闪烁，也保证边缘平滑。逻辑坐标经 world transform 按 DPI 放大到物理像素。
unsafe fn paint_window(hwnd: HWND, hdc: HDC, paint: RECT, state: &mut AppState) {
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let w = (rc.right - rc.left).max(1);
    let h = (rc.bottom - rc.top).max(1);

    // 每窗口 DPI（Per-Monitor）：96=100%，144=150%，192=200%。
    let dpi = GetDpiForWindow(hwnd);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };

    // 尺寸不变时复用整窗位图，避免 hot 切换反复分配和释放约 width*height*4 字节。
    let rebuilt = state.back_buffer_size != (w, h);
    if rebuilt {
        state.back_buffer = OffscreenBitmap::new(w, h);
        state.back_buffer_size = if state.back_buffer.is_some() {
            (w, h)
        } else {
            (0, 0)
        };
    }
    let Some(off) = state.back_buffer.as_ref() else {
        return;
    };

    // 持久 Graphics 每帧先恢复基础状态，避免 DPI 变换和裁剪逐次累积。
    gp::GdipResetWorldTransform(off.graphics());
    gp::GdipResetClip(off.graphics());
    let mut cv = GdiCanvas::with_cache(off.graphics(), &mut state.image_cache);
    cv.set_dpi_scale(scale);

    let lw = w as f32 / scale;
    let lh = h as f32 / scale;
    let physical_dirty = if rebuilt {
        RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        }
    } else {
        RECT {
            left: paint.left.clamp(0, w),
            top: paint.top.clamp(0, h),
            right: paint.right.clamp(0, w),
            bottom: paint.bottom.clamp(0, h),
        }
    };
    if physical_dirty.right <= physical_dirty.left || physical_dirty.bottom <= physical_dirty.top {
        return;
    }
    let dirty = Rect::new(
        physical_dirty.left as f32 / scale,
        physical_dirty.top as f32 / scale,
        (physical_dirty.right - physical_dirty.left) as f32 / scale,
        (physical_dirty.bottom - physical_dirty.top) as f32 / scale,
    );

    // 先恢复脏区背景，再绘制相交的父链和控件分支；画布裁剪防止越界覆盖缓存。
    cv.save();
    cv.clip_rect(dirty);
    cv.fill_rect(dirty, Color::WHITE);
    if rebuilt || state.layout_dirty {
        layout_node(state.root.as_mut(), Rect::new(0.0, 0.0, lw, lh), &cv);
        state.layout_dirty = false;
    }
    paint_tree_in_rect(state.root.as_ref(), &mut cv, dirty);
    state
        .disp
        .paint_overlays(&mut cv, flexui_core::Size::new(lw, lh));
    cv.restore();

    // 只把 rcPaint 对应区域从缓存提交到窗口。
    let pw = physical_dirty.right - physical_dirty.left;
    let ph = physical_dirty.bottom - physical_dirty.top;
    let mut gw: *mut gp::GpGraphics = null_mut();
    if gp::GdipCreateFromHDC(hdc, &mut gw) == 0 && !gw.is_null() {
        gp::GdipDrawImageRectRectI(
            gw,
            off.image(),
            physical_dirty.left,
            physical_dirty.top,
            pw,
            ph,
            physical_dirty.left,
            physical_dirty.top,
            pw,
            ph,
            UNIT_PIXEL,
            null(),
            0,
            null_mut(),
        );
        gp::GdipDeleteGraphics(gw);
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;
    use flexui_core::{Edit, HitPolicy, Panel};

    #[test]
    fn edit_hit_uses_text_cursor() {
        let mut root = Panel::new();
        root.base_mut().rect = Rect::new(0.0, 0.0, 200.0, 100.0);

        let mut edit = Edit::new();
        edit.base_mut().rect = Rect::new(20.0, 20.0, 120.0, 36.0);
        root.base_mut().children.push(Box::new(edit));

        assert!(point_over_edit(&root, Point::new(30.0, 30.0)));
        assert!(!point_over_edit(&root, Point::new(180.0, 80.0)));
        root.base_mut().children[0].base_mut().enabled = false;
        assert!(!point_over_edit(&root, Point::new(30.0, 30.0)));
    }

    #[test]
    fn transparent_overlay_does_not_hide_edit_cursor() {
        let mut root = Panel::new();
        root.base_mut().rect = Rect::new(0.0, 0.0, 200.0, 100.0);

        let mut edit = Edit::new();
        edit.base_mut().rect = Rect::new(20.0, 20.0, 120.0, 36.0);
        root.base_mut().children.push(Box::new(edit));

        let mut overlay = Panel::new();
        overlay.base_mut().rect = Rect::new(0.0, 0.0, 200.0, 100.0);
        overlay.base_mut().hit = HitPolicy::Transparent;
        root.base_mut().children.push(Box::new(overlay));

        assert!(point_over_edit(&root, Point::new(30.0, 30.0)));
    }
}
