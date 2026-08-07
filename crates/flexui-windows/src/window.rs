//! Win32 窗口管理与消息循环（L1，Windows）。
//!
//! RegisterClassW + CreateWindowExW 建窗，WndProc 分发消息：WM_PAINT 用 GDI+ 自绘，
//! 鼠标/键盘消息翻译成 flexui 的 `Event` 交给 `Dispatcher`。与 macOS 后端对位。

use std::ptr::{null, null_mut};

use flexui_core::{layout_node, paint_tree, Color, Dispatcher, Event, MouseButton, Node, Rect};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, HDC, PAINTSTRUCT};
use windows_sys::Win32::Graphics::GdiPlus as gp;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::canvas::GdiCanvas;
use crate::gdiplus::OffscreenBitmap;

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

/// 窗口内部状态：控件树 + 分发器（通过窗口 USERDATA 关联到 HWND）。
struct AppState {
    root: Node,
    disp: Dispatcher,
}

/// UTF-8 → NUL 结尾 UTF-16。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 启动应用：建窗 + 进入消息循环（阻塞直到窗口关闭）。
pub fn run(config: WindowConfig, root: Node, disp: Dispatcher) {
    unsafe {
        // 开启 Per-Monitor V2 DPI 感知（清单已声明；此调用作为兜底，二者一致）。
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance = GetModuleHandleW(null());
        let class_name = wide("FlexUiWindowClass");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
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

        let title = wide(&config.title);
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
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
        let state = Box::into_raw(Box::new(AppState { root, disp }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);

        ShowWindow(hwnd, SW_SHOW);

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

/// 从 lparam 解析鼠标坐标（客户区，左上原点）。
fn mouse_pos(lparam: LPARAM) -> flexui_core::Point {
    let x = (lparam & 0xFFFF) as u16 as i16 as f32;
    let y = ((lparam >> 16) & 0xFFFF) as u16 as i16 as f32;
    flexui_core::Point::new(x, y)
}

/// 分发事件并按需触发重绘。
unsafe fn dispatch(hwnd: HWND, state: *mut AppState, ev: Event) {
    if state.is_null() {
        return;
    }
    let st = &mut *state;
    st.disp.handle(st.root.as_mut(), &ev);
    if st.disp.take_redraw() {
        InvalidateRect(hwnd, null(), 0);
    }
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
            dispatch(hwnd, state, Event::MouseMove { pos: mouse_pos(lparam) });
            0
        }
        WM_LBUTTONDOWN => {
            dispatch(
                hwnd,
                state,
                Event::MouseDown {
                    pos: mouse_pos(lparam),
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
                    pos: mouse_pos(lparam),
                    button: MouseButton::Left,
                },
            );
            0
        }
        WM_CHAR => {
            // 退格作为 KeyDown(8)，其余作为 Char。
            let code = wparam as u32;
            let ev = if code == 8 {
                Event::KeyDown { key: 8 }
            } else if let Some(ch) = char::from_u32(code) {
                Event::Char { ch }
            } else {
                return 0;
            };
            dispatch(hwnd, state, ev);
            0
        }
        WM_SIZE => {
            let w = (lparam & 0xFFFF) as u16 as f32;
            let h = ((lparam >> 16) & 0xFFFF) as u16 as f32;
            dispatch(hwnd, state, Event::WindowResized { width: w, height: h });
            InvalidateRect(hwnd, null(), 0);
            0
        }
        // 背景擦除由我们的双缓冲全覆盖，拦截以消除闪烁。
        WM_ERASEBKGND => 1,
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
