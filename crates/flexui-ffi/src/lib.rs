//! flexui-ffi：C ABI 导出（L6，预留 R6）。
//!
//! 采用「不透明句柄 + 函数 + 回调指针」形态，所有入口用 `catch_unwind` 防止
//! panic 穿越 C 边界。一期提供最小可用集：版本、XML 校验、点击回调、运行。

// C ABI 入口按惯例暴露安全签名（裸指针参数在内部经空指针检查后解引用）。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use flexui_core::{visit_all_mut, WidgetRole};
use flexui_xml::{load_str, Context};

// 按平台选择后端（编译期二选一）。
#[cfg(target_os = "macos")]
use flexui_macos as backend;
#[cfg(target_os = "windows")]
use flexui_windows as backend;

/// 库版本（主*10000 + 次*100 + 补丁）。
#[no_mangle]
pub extern "C" fn flex_version() -> u32 {
    1 // 0.1.0 → 简单返回 1
}

/// 把 C 字符串转 Rust &str（失败返回 None）。
unsafe fn cstr<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    CStr::from_ptr(p).to_str().ok()
}

/// 校验并构建一段 XML，返回根节点的顶层子节点数量；出错返回负数。
///
/// 这是一个「非阻塞」入口，便于在无 GUI 环境验证 C ABI 边界。
/// -1: 空指针/编码错误；-2: 解析/构建失败；-3: 内部 panic。
#[no_mangle]
pub extern "C" fn flex_load_check(xml: *const c_char) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(xml) = (unsafe { cstr(xml) }) else {
            return -1;
        };
        let ctx = Context::new();
        match load_str(xml, &ctx) {
            Ok(res) => res.root.base().children.len() as c_int,
            Err(_) => -2,
        }
    }));
    result.unwrap_or(-3)
}

// —— 点击回调桥接 ——
type ClickFn = extern "C" fn(name: *const c_char, user: *mut c_void);

thread_local! {
    static CLICK_CB: RefCell<Option<(ClickFn, usize)>> = const { RefCell::new(None) };
}

/// 注册全局点击回调：任意「有 name 的按钮」被点击时回调 `cb(name, user)`。
#[no_mangle]
pub extern "C" fn flex_set_click_callback(cb: Option<ClickFn>, user: *mut c_void) {
    CLICK_CB.with(|c| {
        *c.borrow_mut() = cb.map(|f| (f, user as usize));
    });
}

/// 触发已注册的点击回调（内部用）。
fn fire_click(name: &str) {
    if let Ok(cname) = CString::new(name) {
        CLICK_CB.with(|c| {
            if let Some((cb, user)) = *c.borrow() {
                cb(cname.as_ptr(), user as *mut c_void);
            }
        });
    }
}

/// 用 XML 描述启动应用并进入主事件循环（阻塞）。0 成功，负数见错误码。
///
/// 会为所有「有 name 的按钮」自动挂接点击回调（转发到 flex_set_click_callback 注册的函数）。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[no_mangle]
pub extern "C" fn flex_run_xml(
    title: *const c_char,
    width: c_int,
    height: c_int,
    xml: *const c_char,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let (Some(title), Some(xml)) = (unsafe { cstr(title) }, unsafe { cstr(xml) }) else {
            return -1;
        };
        let ctx = Context::new();
        let res = match load_str(xml, &ctx) {
            Ok(r) => r,
            Err(_) => return -2,
        };
        let mut root = res.root;

        // 为有 name 的按钮挂接点击回调（C 侧通过 name 区分）。
        visit_all_mut(root.as_mut(), &mut |w| {
            let b = w.base_mut();
            if b.role == WidgetRole::Button {
                if let Some(name) = b.name.clone() {
                    b.on_click = Some(Box::new(move || fire_click(&name)));
                }
            }
        });

        let mut disp = flexui_core::Dispatcher::new();
        for (group, tabbox) in res.bindings {
            disp.bind_tab(group, tabbox);
        }
        backend::run(
            backend::WindowConfig {
                title: title.to_string(),
                width: width as f32,
                height: height as f32,
            },
            root,
            disp,
        );
        0
    }));
    result.unwrap_or(-3)
}

/// 无可用后端平台的占位。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[no_mangle]
pub extern "C" fn flex_run_xml(
    _title: *const c_char,
    _width: c_int,
    _height: c_int,
    _xml: *const c_char,
) -> c_int {
    -100 // 该平台后端未实现
}
