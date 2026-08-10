//! Win32 窗口管理与消息循环（L1，Windows）。
//!
//! RegisterClassW + CreateWindowExW 建窗，WndProc 分发消息：WM_PAINT 用 GDI+ 自绘，
//! 鼠标/键盘消息翻译成 flexui 的 `Event` 交给 `Dispatcher`。与 macOS 后端对位。

use std::ptr::{null, null_mut};

use flexui_core::event::keys;
use flexui_core::{
    hit_test, layout_node, paint_tree, Color, Dispatcher, Event, Mods, MouseButton, Node, Point,
    Rect, TitlebarMode, WindowConfig, WindowCtx, WindowDelegate, WindowHandle,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, ScreenToClient, HDC, PAINTSTRUCT,
};
use windows_sys::Win32::Graphics::GdiPlus as gp;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::Ime::{
    ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, ImmSetCompositionWindow,
    COMPOSITIONFORM, CFS_POINT, GCS_COMPSTR, GCS_RESULTSTR,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::canvas::GdiCanvas;
use crate::gdiplus::OffscreenBitmap;

/// 顶部可拖动条高度（逻辑像素）。
const DRAG_STRIP: f32 = 40.0;

/// 窗口内部状态：控件树 + 分发器 + 窗口委托（通过窗口 USERDATA 关联到 HWND）。
struct AppState {
    root: Node,
    disp: Dispatcher,
    delegate: Box<dyn WindowDelegate>,
    /// 无边框（自绘标题栏）模式：启用自定义拖动/命中。
    frameless: bool,
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
        unsafe { ShowWindow(self.hwnd, SW_MINIMIZE) };
    }
    fn maximize(&mut self) {
        unsafe { ShowWindow(self.hwnd, SW_MAXIMIZE) };
    }
    fn restore(&mut self) {
        unsafe { ShowWindow(self.hwnd, SW_RESTORE) };
    }
}

/// UTF-8 → NUL 结尾 UTF-16。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 启动应用：建窗 + 进入消息循环（阻塞直到窗口关闭）。
///
/// 由 facade 的 `Window` 驱动调用；`delegate` 承载 on_init/on_activate 等窗口钩子。
pub fn run(config: WindowConfig, root: Node, disp: Dispatcher, delegate: Box<dyn WindowDelegate>) {
    unsafe {
        // 开启 Per-Monitor V2 DPI 感知（清单已声明；此调用作为兜底，二者一致）。
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance = GetModuleHandleW(null());
        let class_name = wide("FlexUiWindowClass");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wc);

        // 窗口样式。无边框模式用 WS_POPUP 去系统标题栏；保留 WS_THICKFRAME/最大化
        // 以维持可缩放与系统 Aero Snap（拖到屏幕边缘贴靠）。
        let frameless = config.titlebar != TitlebarMode::System;
        let mut style = if frameless {
            WS_POPUP | WS_VISIBLE | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_THICKFRAME
        } else {
            WS_OVERLAPPEDWINDOW | WS_VISIBLE
        };
        if !config.resizable {
            style &= !(WS_THICKFRAME | WS_MAXIMIZEBOX);
        }

        let title = wide(&config.title);
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            config.width as i32,
            config.height as i32,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        );
        if hwnd.is_null() {
            eprintln!("[flexui] CreateWindowExW 失败");
            return;
        }

        // 关联 AppState 到窗口。
        let state = Box::into_raw(Box::new(AppState {
            root,
            disp,
            delegate,
            frameless,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);

        ShowWindow(hwnd, SW_SHOW);

        // 光标闪烁定时器（530ms）。
        SetTimer(hwnd, 1, 530, None);

        // 窗口就绪后触发 on_init（≈ InitWindow）。
        {
            let st = &mut *state;
            let mut handle = WinWindowHandle { hwnd };
            let mut ctx = WindowCtx::new(st.root.as_mut(), &mut handle);
            st.delegate.on_init(&mut ctx);
            InvalidateRect(hwnd, null(), 0);
        }

        // 消息循环。
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
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
unsafe fn dispatch(hwnd: HWND, state: *mut AppState, ev: Event) {
    if state.is_null() {
        return;
    }
    let st = &mut *state;
    st.disp.handle(st.root.as_mut(), &ev);
    let need = st.disp.take_redraw();
    let dirty = st.disp.take_dirty();
    let acts = st.disp.take_activations();
    let doubles = st.disp.take_double_clicks();
    let contexts = st.disp.take_context_clicks();

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
    }
    // 整窗重绘优先，否则只失效脏矩形（BeginPaint 的 HDC 会裁剪到更新区域）。
    if need || !acts.is_empty() || !doubles.is_empty() {
        InvalidateRect(hwnd, null(), 0);
    } else if let Some(r) = dirty {
        let rc = to_physical_rect(hwnd, r);
        InvalidateRect(hwnd, &rc, 0);
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
        ImmGetCompositionStringW(himc, gcs, buf.as_mut_ptr() as *mut core::ffi::c_void, n as u32);
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
unsafe fn with_focus_base(
    state: *mut AppState,
    f: impl FnOnce(&mut flexui_core::Base),
) -> Option<Rect> {
    if state.is_null() {
        return None;
    }
    let st = &mut *state;
    let id = st.disp.focus()?;
    let w = flexui_core::find_mut_by_id(st.root.as_mut(), id)?;
    let b = w.base_mut();
    f(b);
    Some(b.rect)
}

/// 设置焦点控件的 IME 组合串并失效其区域。
unsafe fn set_marked_on_focus(hwnd: HWND, state: *mut AppState, text: &str) {
    let owned = text.to_string();
    if let Some(r) = with_focus_base(state, move |b| b.marked = owned) {
        let rc = to_physical_rect(hwnd, r);
        InvalidateRect(hwnd, &rc, 0);
    }
}

/// 清除焦点控件的 IME 组合串并失效其区域。
unsafe fn clear_marked_on_focus(hwnd: HWND, state: *mut AppState) {
    if let Some(r) = with_focus_base(state, |b| b.marked.clear()) {
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
    let Some(w) = flexui_core::find_mut_by_id(st.root.as_mut(), id) else { return };
    let r = w.base().rect;
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
        rcArea: RECT { left: 0, top: 0, right: 0, bottom: 0 },
    };
    ImmSetCompositionWindow(himc, &form);
    ImmReleaseContext(hwnd, himc);
}

/// 窗口过程。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let state = app_state(hwnd);
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !state.is_null() {
                paint_window(hwnd, hdc, &mut *state);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_MOUSEMOVE => {
            dispatch(hwnd, state, Event::MouseMove { pos: mouse_pos(hwnd, lparam) });
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
            dispatch(
                hwnd,
                state,
                Event::MouseUp {
                    pos: mouse_pos(hwnd, lparam),
                    button: MouseButton::Left,
                },
            );
            0
        }
        // 方向/Home/End/Delete 等特殊键（不产生 WM_CHAR）→ 平台无关键码。
        WM_KEYDOWN => {
            let key = match wparam as u32 {
                0x25 => keys::LEFT,
                0x27 => keys::RIGHT,
                0x26 => keys::UP,
                0x28 => keys::DOWN,
                0x24 => keys::HOME,
                0x23 => keys::END,
                0x2E => keys::DELETE,
                _ => return DefWindowProcW(hwnd, msg, wparam, lparam), // 其余交系统（保留 WM_CHAR）
            };
            dispatch(hwnd, state, Event::KeyDown { key, mods: Mods::default() });
            0
        }
        WM_RBUTTONUP => {
            dispatch(
                hwnd,
                state,
                Event::MouseUp { pos: mouse_pos(hwnd, lparam), button: MouseButton::Right },
            );
            0
        }
        WM_LBUTTONDBLCLK => {
            dispatch(hwnd, state, Event::DoubleClick { pos: mouse_pos(hwnd, lparam) });
            0
        }
        WM_CHAR => {
            // 退格→KeyDown(8)，Tab→KeyDown(9) 焦点遍历，其余作为 Char。
            let code = wparam as u32;
            let ev = if code == 8 {
                Event::KeyDown { key: 8, mods: Mods::default() }
            } else if code == 9 {
                Event::KeyDown { key: 9, mods: Mods::default() }
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
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16;
            let dy = delta as f32 / 30.0; // 一格(120) ≈ 4px 逻辑
            // lparam 为屏幕坐标 → 客户区 → 逻辑坐标。
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
            let w = (lparam & 0xFFFF) as u16 as f32;
            let h = ((lparam >> 16) & 0xFFFF) as u16 as f32;
            dispatch(hwnd, state, Event::WindowResized { width: w, height: h });
            InvalidateRect(hwnd, null(), 0);
            0
        }
        // 光标闪烁定时器：切换焦点控件 caret 相位，只失效其区域。
        WM_TIMER => {
            if !state.is_null() {
                let st = &mut *state;
                if let Some(r) = st.disp.blink(st.root.as_mut()) {
                    let rc = to_physical_rect(hwnd, r);
                    InvalidateRect(hwnd, &rc, 0);
                }
            }
            0
        }
        // 背景擦除由我们的双缓冲全覆盖，拦截以消除闪烁。
        WM_ERASEBKGND => 1,
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
                DestroyWindow(hwnd);
            }
            0
        }
        // 无边框：顶部空白条可拖动（有实体控件处放行给控件），实现自绘标题栏拖动 + Aero Snap。
        WM_NCHITTEST if !state.is_null() && (*state).frameless => {
            let default = DefWindowProcW(hwnd, msg, wparam, lparam);
            if default != HTCLIENT as isize {
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
            if lp.y < DRAG_STRIP && hit_test(st.root.as_ref(), lp).is_none() {
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
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_DESTROY => {
            if !state.is_null() {
                drop(Box::from_raw(state));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// WM_PAINT 期间：双缓冲 + DPI 缩放绘制整棵控件树。
///
/// 先画到 32bpp 离屏位图（抗锯齿/ClearType 在位图上效果最佳），再整块 blit 到窗口，
/// 既消除 resize 闪烁，也保证边缘平滑。逻辑坐标经 world transform 按 DPI 放大到物理像素。
unsafe fn paint_window(hwnd: HWND, hdc: HDC, state: &mut AppState) {
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let w = (rc.right - rc.left).max(1);
    let h = (rc.bottom - rc.top).max(1);

    // 每窗口 DPI（Per-Monitor）：96=100%，144=150%，192=200%。
    let dpi = GetDpiForWindow(hwnd);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };

    // 离屏位图按物理像素尺寸；绘制经 DPI 缩放后填满它。
    let Some(off) = OffscreenBitmap::new(w, h) else {
        return;
    };
    let mut cv = GdiCanvas::new(off.graphics());
    cv.clear(Color::WHITE); // 不透明底，供未覆盖区域与 ClearType 使用
    cv.set_dpi_scale(scale);

    // 布局用逻辑像素（= 物理 / scale）。
    let lw = w as f32 / scale;
    let lh = h as f32 / scale;
    layout_node(state.root.as_mut(), Rect::new(0.0, 0.0, lw, lh), &cv);
    paint_tree(state.root.as_ref(), &mut cv);

    // 整块 blit 到窗口（1:1 物理像素）。
    let mut gw: *mut gp::GpGraphics = null_mut();
    if gp::GdipCreateFromHDC(hdc, &mut gw) == 0 && !gw.is_null() {
        gp::GdipDrawImageRectI(gw, off.image(), 0, 0, w, h);
        gp::GdipDeleteGraphics(gw);
    }
}
