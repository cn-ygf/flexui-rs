//! flexui-ffi：C ABI 导出（L6，预留 R6）。
//!
//! 采用「不透明句柄 + 函数 + 回调指针」形态，所有入口用 `catch_unwind` 防止
//! panic 穿越 C 边界。一期提供最小可用集：版本、XML 校验、点击回调、运行。

// C ABI 入口按惯例暴露安全签名（裸指针参数在内部经空指针检查后解引用）。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use flexui_core::{
    visit_all_mut, ControlEvent, MainProxy, WidgetProperty, WidgetRole, WindowCtx, WindowDelegate,
    WindowEvent,
};
use flexui_xml::{load_str, Context};

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use flexui_core::dialog::{DialogKind, FileDialog};

// 按平台选择后端（编译期三选一）。
#[cfg(target_os = "macos")]
use flexui_macos as backend;
#[cfg(target_os = "windows")]
use flexui_windows as backend;
#[cfg(target_os = "linux")]
use flexui_linux as backend;

/// 库版本（主*10000 + 次*100 + 补丁）。
#[no_mangle]
pub extern "C" fn flex_version() -> u32 {
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    let major = parts.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|v| v.split('-').next())
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    major * 10_000 + minor * 100 + patch
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
pub extern "C" fn flex_set_click_callback(
    cb: Option<extern "C" fn(name: *const c_char, user: *mut c_void)>,
    user: *mut c_void,
) {
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

// —— 窗口上下文（FlexCtx）操作：在回调里通过不透明指针访问/控制控件与窗口 ——

/// 把 C 侧不透明指针还原为 &mut WindowCtx（仅在回调期间有效）。
unsafe fn ctx_from<'a>(p: *mut c_void) -> Option<&'a mut WindowCtx<'a>> {
    if p.is_null() {
        None
    } else {
        Some(&mut *(p as *mut WindowCtx))
    }
}

/// 把字符串写入调用方缓冲（含 NUL）；返回写入的字节数（不含 NUL），缓冲不足或出错返回 -1。
fn write_str(s: &str, out: *mut c_char, out_len: c_int) -> c_int {
    if out.is_null() || out_len <= 0 {
        return -1;
    }
    let bytes = s.as_bytes();
    let cap = out_len as usize;
    if bytes.len() + 1 > cap {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, bytes.len());
        *out.add(bytes.len()) = 0;
    }
    bytes.len() as c_int
}

/// 设置某具名控件的文本。
#[no_mangle]
pub extern "C" fn flex_ctx_set_text(ctx: *mut c_void, name: *const c_char, text: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n), Some(t)) = (ctx_from(ctx), cstr(name), cstr(text)) {
            ctx.set_text(n, t);
        }
    }));
}

/// 读取某具名控件的文本到 out 缓冲；返回长度，未找到/出错返回 -1。
#[no_mangle]
pub extern "C" fn flex_ctx_get_text(
    ctx: *mut c_void,
    name: *const c_char,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return -1;
        };
        match ctx.with(n, |w| w.base().text.clone()) {
            Some(text) => write_str(&text, out, out_len),
            None => -1,
        }
    }))
    .unwrap_or(-1)
}

/// 读取某具名控件的 selected（CheckBox/Radio）：1=选中，0=未选，-1=未找到。
#[no_mangle]
pub extern "C" fn flex_ctx_is_selected(ctx: *mut c_void, name: *const c_char) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return -1;
        };
        match ctx.is_selected(n) {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    }))
    .unwrap_or(-1)
}

/// 设置某具名控件是否可用。
#[no_mangle]
pub extern "C" fn flex_ctx_set_enabled(ctx: *mut c_void, name: *const c_char, enabled: c_int) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) {
            ctx.set_enabled(n, enabled != 0);
        }
    }));
}

/// 设置窗口标题。
#[no_mangle]
pub extern "C" fn flex_ctx_set_title(ctx: *mut c_void, title: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(t)) = (ctx_from(ctx), cstr(title)) {
            ctx.set_title(t);
        }
    }));
}

/// 请求关闭窗口。
#[no_mangle]
pub extern "C" fn flex_ctx_close(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.close();
        }
    }));
}

// —— 值 / 选择 / 可见性等状态读写（补全 WindowCtx 覆盖）——

/// 设置进度条 / 滑块等控件的数值。
#[no_mangle]
pub extern "C" fn flex_ctx_set_value(ctx: *mut c_void, name: *const c_char, value: f32) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) {
            ctx.set_value(n, value);
        }
    }));
}

/// 读控件数值到 *out；返回 1=已取到，0=未找到/出错。
#[no_mangle]
pub extern "C" fn flex_ctx_get_value(ctx: *mut c_void, name: *const c_char, out: *mut f32) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return 0;
        };
        match (out.is_null(), ctx.value(n)) {
            (false, Some(v)) => {
                *out = v;
                1
            }
            _ => 0,
        }
    }))
    .unwrap_or(0)
}

/// 设置 CheckBox / Radio 等的选中态。
#[no_mangle]
pub extern "C" fn flex_ctx_set_selected(ctx: *mut c_void, name: *const c_char, selected: c_int) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) {
            ctx.set_selected(n, selected != 0);
        }
    }));
}

/// 设置下拉 / 分段等的选中项索引（负数忽略）。
#[no_mangle]
pub extern "C" fn flex_ctx_set_selected_index(ctx: *mut c_void, name: *const c_char, index: c_int) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) {
            if index >= 0 {
                ctx.set_selected_index(n, index as usize);
            }
        }
    }));
}

/// 读选中项索引到 *out；返回 1=已取到，0=未找到。
#[no_mangle]
pub extern "C" fn flex_ctx_get_selected_index(
    ctx: *mut c_void,
    name: *const c_char,
    out: *mut c_int,
) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return 0;
        };
        match (out.is_null(), ctx.selected_index(n)) {
            (false, Some(i)) => {
                *out = i as c_int;
                1
            }
            _ => 0,
        }
    }))
    .unwrap_or(0)
}

/// 设置控件可见性。
#[no_mangle]
pub extern "C" fn flex_ctx_set_visible(ctx: *mut c_void, name: *const c_char, visible: c_int) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) {
            ctx.set_visible(n, visible != 0);
        }
    }));
}

/// 读控件可见性：1=可见，0=隐藏，-1=未找到。
#[no_mangle]
pub extern "C" fn flex_ctx_is_visible(ctx: *mut c_void, name: *const c_char) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return -1;
        };
        match ctx.is_visible(n) {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    }))
    .unwrap_or(-1)
}

/// 读控件可用性：1=可用，0=禁用，-1=未找到。
#[no_mangle]
pub extern "C" fn flex_ctx_is_enabled(ctx: *mut c_void, name: *const c_char) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return -1;
        };
        match ctx.is_enabled(n) {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    }))
    .unwrap_or(-1)
}

/// 设置输入框占位提示文本。
#[no_mangle]
pub extern "C" fn flex_ctx_set_placeholder(
    ctx: *mut c_void,
    name: *const c_char,
    text: *const c_char,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n), Some(t)) = (ctx_from(ctx), cstr(name), cstr(text)) {
            ctx.set_property(n, WidgetProperty::Placeholder(t.to_string()));
        }
    }));
}

/// 通知数据源控件（列表 / 虚拟列表）刷新数据；返回 1=已处理，0=未找到。
#[no_mangle]
pub extern "C" fn flex_ctx_refresh_data(ctx: *mut c_void, name: *const c_char) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return 0;
        };
        ctx.refresh_data(n) as c_int
    }))
    .unwrap_or(0)
}

// —— 动态子节点：从 XML 片段增删 ——

/// 解析一段 XML 片段并追加为具名容器的子节点；返回 1=成功，0=未找到容器/解析失败。
#[no_mangle]
pub extern "C" fn flex_ctx_add_child_xml(
    ctx: *mut c_void,
    name: *const c_char,
    xml: *const c_char,
) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n), Some(x)) = (ctx_from(ctx), cstr(name), cstr(xml)) else {
            return 0;
        };
        let build_ctx = Context::new();
        let Ok(res) = load_str(x, &build_ctx) else {
            return 0;
        };
        ctx.add_child(n, res.root) as c_int
    }))
    .unwrap_or(0)
}

/// 清空具名容器的所有子节点；返回 1=已处理，0=未找到。
#[no_mangle]
pub extern "C" fn flex_ctx_clear_children(ctx: *mut c_void, name: *const c_char) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(n)) = (ctx_from(ctx), cstr(name)) else {
            return 0;
        };
        ctx.clear_children(n).is_some() as c_int
    }))
    .unwrap_or(0)
}

// —— 窗口控制 ——

/// 显示窗口。
#[no_mangle]
pub extern "C" fn flex_ctx_show(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.show();
        }
    }));
}

/// 隐藏窗口。
#[no_mangle]
pub extern "C" fn flex_ctx_hide(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.hide();
        }
    }));
}

/// 最小化窗口。
#[no_mangle]
pub extern "C" fn flex_ctx_minimize(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.minimize();
        }
    }));
}

/// 最大化窗口。
#[no_mangle]
pub extern "C" fn flex_ctx_maximize(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.maximize();
        }
    }));
}

/// 还原窗口（取消最小/最大化）。
#[no_mangle]
pub extern "C" fn flex_ctx_restore(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.restore();
        }
    }));
}

/// 退出整个应用（结束事件循环）。
#[no_mangle]
pub extern "C" fn flex_ctx_quit(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.quit();
        }
    }));
}

/// 请求整窗重绘。
#[no_mangle]
pub extern "C" fn flex_ctx_request_redraw(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.request_redraw();
        }
    }));
}

/// 请求重新布局。
#[no_mangle]
pub extern "C" fn flex_ctx_request_layout(ctx: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(ctx) = ctx_from(ctx) {
            ctx.request_layout();
        }
    }));
}

// —— 国际化 ——

/// 切换界面语言（BCP-47，如 "zh-CN"/"en"）；返回 1=成功，0=失败。
#[no_mangle]
pub extern "C" fn flex_ctx_set_locale(ctx: *mut c_void, locale: *const c_char) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(ctx), Some(loc)) = (ctx_from(ctx), cstr(locale)) else {
            return 0;
        };
        ctx.set_locale(loc).is_ok() as c_int
    }))
    .unwrap_or(0)
}

/// 把具名控件的文本绑定到某本地化资源键（随语言切换自动更新）。
#[no_mangle]
pub extern "C" fn flex_ctx_set_localized_text(
    ctx: *mut c_void,
    name: *const c_char,
    resource_key: *const c_char,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let (Some(ctx), Some(n), Some(key)) = (ctx_from(ctx), cstr(name), cstr(resource_key)) {
            ctx.set_localized_text(n, key);
        }
    }));
}

// —— UI 线程投递：句柄可由初始化回调取得，再移动到工作线程使用 ——

/// C 侧持有的不透明 UI 线程投递句柄。
pub struct FlexMainProxy {
    proxy: MainProxy,
}

/// 从窗口回调取得 UI 线程投递句柄；调用方负责用 `flex_main_proxy_free` 释放。
#[no_mangle]
pub extern "C" fn flex_ctx_main_proxy(ctx: *mut c_void) -> *mut FlexMainProxy {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let Some(proxy) = ctx_from(ctx).and_then(|ctx| ctx.main_proxy()) else {
            return std::ptr::null_mut();
        };
        Box::into_raw(Box::new(FlexMainProxy { proxy }))
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// 克隆投递句柄，便于多个工作线程分别持有；返回值需要单独释放。
#[no_mangle]
pub extern "C" fn flex_main_proxy_clone(proxy: *const FlexMainProxy) -> *mut FlexMainProxy {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let Some(proxy) = proxy.as_ref() else {
            return std::ptr::null_mut();
        };
        Box::into_raw(Box::new(FlexMainProxy {
            proxy: proxy.proxy.clone(),
        }))
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// 投递 C 回调到所属窗口的 UI 线程；1 表示已接受，0 表示窗口已关闭或参数无效。
#[no_mangle]
pub extern "C" fn flex_main_proxy_post(
    proxy: *const FlexMainProxy,
    task: Option<extern "C" fn(ctx: *mut c_void, user: *mut c_void)>,
    user: *mut c_void,
) -> c_int {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let (Some(proxy), Some(task)) = (proxy.as_ref(), task) else {
            return 0;
        };
        let user = user as usize;
        if proxy.proxy.post(move |ctx| {
            task(ctx as *mut _ as *mut c_void, user as *mut c_void);
        }) {
            1
        } else {
            0
        }
    }))
    .unwrap_or(0)
}

/// 释放一个投递句柄。传 NULL 无操作。
#[no_mangle]
pub extern "C" fn flex_main_proxy_free(proxy: *mut FlexMainProxy) {
    if !proxy.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(proxy));
        }));
    }
}

// —— 窗口委托桥接：C 侧函数指针集 ——

/// C 侧窗口状态事件编号。
pub const FLEX_WINDOW_MINIMIZED: c_int = 1;
pub const FLEX_WINDOW_MAXIMIZED: c_int = 2;
pub const FLEX_WINDOW_RESTORED: c_int = 3;

/// C 侧控件事件类型编号（用于 `FlexDelegate::on_control_event`）。
/// 各类型用到 on_control_event 的哪些参数：
///   HOVER/PRESSED/FOCUS/SELECTED → i0=0/1；SELECTION → i0=索引(无=-1)；
///   TEXT → text；VALUE → f0；SCROLL → f0=横偏移, f1=纵偏移。
pub const FLEX_CTRL_HOVER_CHANGED: c_int = 1;
pub const FLEX_CTRL_PRESSED_CHANGED: c_int = 2;
pub const FLEX_CTRL_FOCUS_CHANGED: c_int = 3;
pub const FLEX_CTRL_TEXT_CHANGED: c_int = 4;
pub const FLEX_CTRL_SELECTED_CHANGED: c_int = 5;
pub const FLEX_CTRL_SELECTION_CHANGED: c_int = 6;
pub const FLEX_CTRL_VALUE_CHANGED: c_int = 7;
pub const FLEX_CTRL_SCROLL_CHANGED: c_int = 8;

/// C 侧窗口委托：各钩子为可空函数指针，ctx 仅在回调期间有效。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlexDelegate {
    pub on_before_init: Option<extern "C" fn(ctx: *mut c_void, user: *mut c_void)>,
    pub on_init: Option<extern "C" fn(ctx: *mut c_void, user: *mut c_void)>,
    pub on_initialized: Option<extern "C" fn(ctx: *mut c_void, user: *mut c_void)>,
    pub on_click: Option<extern "C" fn(name: *const c_char, ctx: *mut c_void, user: *mut c_void)>,
    /// 通用控件事件（复选框勾选、滑块/进度值变化、文本变化、下拉选择、滚动等）。
    /// `ev_type` 见 `FLEX_CTRL_*`；按类型读取 i0/f0/f1/text（其余为 0/NULL）。
    pub on_control_event: Option<
        extern "C" fn(
            name: *const c_char,
            ev_type: c_int,
            i0: c_int,
            f0: f32,
            f1: f32,
            text: *const c_char,
            ctx: *mut c_void,
            user: *mut c_void,
        ),
    >,
    pub on_double_click:
        Option<extern "C" fn(name: *const c_char, ctx: *mut c_void, user: *mut c_void)>,
    pub on_context: Option<
        extern "C" fn(name: *const c_char, x: f32, y: f32, ctx: *mut c_void, user: *mut c_void),
    >,
    /// 窗口尺寸变化（逻辑像素）。
    pub on_size: Option<extern "C" fn(width: f32, height: f32, ctx: *mut c_void, user: *mut c_void)>,
    /// 按键（导航/功能键的平台无关键码）。
    pub on_key: Option<extern "C" fn(key: c_int, ctx: *mut c_void, user: *mut c_void)>,
    pub on_window_state: Option<extern "C" fn(state: c_int, ctx: *mut c_void, user: *mut c_void)>,
    /// 返回非 0 允许关闭，0 阻止关闭。
    pub on_closing: Option<extern "C" fn(ctx: *mut c_void, user: *mut c_void) -> c_int>,
    pub on_closed: Option<extern "C" fn(user: *mut c_void)>,
}

/// 把 C 委托适配成 WindowDelegate。
struct CDelegate {
    d: FlexDelegate,
    user: usize,
}

impl CDelegate {
    fn user_ptr(&self) -> *mut c_void {
        self.user as *mut c_void
    }
}

impl WindowDelegate for CDelegate {
    fn on_before_init(&mut self, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_before_init {
            callback(ctx as *mut _ as *mut c_void, self.user_ptr());
        }
    }

    fn on_init(&mut self, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_init {
            callback(ctx as *mut _ as *mut c_void, self.user_ptr());
        }
    }

    fn on_initialized(&mut self, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_initialized {
            callback(ctx as *mut _ as *mut c_void, self.user_ptr());
        }
    }

    fn on_activate(&mut self, name: &str, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_click {
            if let Ok(name) = CString::new(name) {
                callback(name.as_ptr(), ctx as *mut _ as *mut c_void, self.user_ptr());
            }
        }
    }

    fn on_control_event(&mut self, name: &str, event: &ControlEvent, ctx: &mut WindowCtx) {
        let Some(callback) = self.d.on_control_event else {
            return;
        };
        let Ok(cname) = CString::new(name) else {
            return;
        };
        // 把 ControlEvent 摊平成 (ev_type, i0, f0, f1, text)；虚拟列表专属事件本轮不导出。
        let ev_type;
        let mut i0: c_int = 0;
        let mut f0 = 0.0;
        let mut f1 = 0.0;
        let mut text: Option<CString> = None;
        match event {
            ControlEvent::HoverChanged(v) => {
                ev_type = FLEX_CTRL_HOVER_CHANGED;
                i0 = *v as c_int;
            }
            ControlEvent::PressedChanged(v) => {
                ev_type = FLEX_CTRL_PRESSED_CHANGED;
                i0 = *v as c_int;
            }
            ControlEvent::FocusChanged(v) => {
                ev_type = FLEX_CTRL_FOCUS_CHANGED;
                i0 = *v as c_int;
            }
            ControlEvent::TextChanged(s) => {
                ev_type = FLEX_CTRL_TEXT_CHANGED;
                text = CString::new(s.as_str()).ok();
            }
            ControlEvent::SelectedChanged(v) => {
                ev_type = FLEX_CTRL_SELECTED_CHANGED;
                i0 = *v as c_int;
            }
            ControlEvent::SelectionChanged(idx) => {
                ev_type = FLEX_CTRL_SELECTION_CHANGED;
                i0 = idx.map(|i| i as c_int).unwrap_or(-1);
            }
            ControlEvent::ValueChanged(v) => {
                ev_type = FLEX_CTRL_VALUE_CHANGED;
                f0 = *v;
            }
            ControlEvent::ScrollChanged(p) => {
                ev_type = FLEX_CTRL_SCROLL_CHANGED;
                f0 = p.x;
                f1 = p.y;
            }
            // 虚拟列表多选/排序/列变化、帧动画结束等本轮不导出到 C。
            _ => return,
        }
        let text_ptr = text.as_ref().map_or(std::ptr::null(), |t| t.as_ptr());
        callback(
            cname.as_ptr(),
            ev_type,
            i0,
            f0,
            f1,
            text_ptr,
            ctx as *mut _ as *mut c_void,
            self.user_ptr(),
        );
    }

    fn on_double_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_double_click {
            if let Ok(name) = CString::new(name) {
                callback(name.as_ptr(), ctx as *mut _ as *mut c_void, self.user_ptr());
            }
        }
    }

    fn on_size(&mut self, width: f32, height: f32, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_size {
            callback(width, height, ctx as *mut _ as *mut c_void, self.user_ptr());
        }
    }

    fn on_key(&mut self, key: u32, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_key {
            callback(key as c_int, ctx as *mut _ as *mut c_void, self.user_ptr());
        }
    }

    fn on_context(&mut self, name: &str, x: f32, y: f32, ctx: &mut WindowCtx) {
        if let Some(callback) = self.d.on_context {
            if let Ok(name) = CString::new(name) {
                callback(
                    name.as_ptr(),
                    x,
                    y,
                    ctx as *mut _ as *mut c_void,
                    self.user_ptr(),
                );
            }
        }
    }

    fn on_window_event(&mut self, event: &WindowEvent, ctx: &mut WindowCtx) {
        let state = match event {
            WindowEvent::Minimized => FLEX_WINDOW_MINIMIZED,
            WindowEvent::Maximized => FLEX_WINDOW_MAXIMIZED,
            WindowEvent::Restored => FLEX_WINDOW_RESTORED,
            _ => return,
        };
        if let Some(callback) = self.d.on_window_state {
            callback(state, ctx as *mut _ as *mut c_void, self.user_ptr());
        }
    }

    fn on_closing(&mut self, ctx: &mut WindowCtx) -> bool {
        self.d
            .on_closing
            .is_none_or(|callback| callback(ctx as *mut _ as *mut c_void, self.user_ptr()) != 0)
    }

    fn on_closed(&mut self) {
        if let Some(callback) = self.d.on_closed {
            callback(self.user_ptr());
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn run_with_delegate(
    title: *const c_char,
    width: c_int,
    height: c_int,
    xml: *const c_char,
    delegate: Box<dyn WindowDelegate>,
) -> c_int {
    let (Some(title), Some(xml)) = (unsafe { cstr(title) }, unsafe { cstr(xml) }) else {
        return -1;
    };
    let ctx = Context::new();
    let result = match load_str(xml, &ctx) {
        Ok(result) => result,
        Err(_) => return -2,
    };
    let mut disp = flexui_core::Dispatcher::new();
    for (group, tabbox) in result.bindings {
        disp.bind_tab(group, tabbox);
    }
    backend::run(
        flexui_core::WindowConfig::new(title, width as f32, height as f32),
        result.root,
        disp,
        delegate,
    );
    0
}

/// 用 XML + C 委托启动应用（阻塞）。delegate 为 NULL 时等价于 flex_run_xml。
/// 0 成功，负数错误码（-1 参数错、-2 XML 失败、-3 panic、-100 无后端）。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn flex_run(
    title: *const c_char,
    width: c_int,
    height: c_int,
    xml: *const c_char,
    delegate: *const FlexDelegate,
    user: *mut c_void,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let delegate: Box<dyn WindowDelegate> = if delegate.is_null() {
            Box::new(flexui_core::NoopDelegate)
        } else {
            Box::new(CDelegate {
                d: unsafe { *delegate },
                user: user as usize,
            })
        };
        run_with_delegate(title, width, height, xml, delegate)
    }));
    result.unwrap_or(-3)
}

// —— 系统剪贴板 ——

/// 读系统剪贴板文本到 out（含 NUL）；返回长度，空/出错返回 -1。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn flex_clipboard_get_text(out: *mut c_char, out_len: c_int) -> c_int {
    catch_unwind(AssertUnwindSafe(|| match backend::clipboard_get_text() {
        Some(s) => write_str(&s, out, out_len),
        None => -1,
    }))
    .unwrap_or(-1)
}

/// 写系统剪贴板文本。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn flex_clipboard_set_text(text: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        if let Some(t) = cstr(text) {
            backend::clipboard_set_text(t);
        }
    }));
}

// —— 文件对话框 ——

/// C 侧文件对话框配置。filter_exts 为逗号分隔扩展名（如 "png,jpg"），可空。
#[repr(C)]
pub struct FlexFileDialog {
    pub title: *const c_char,
    pub default_dir: *const c_char,
    pub default_name: *const c_char,
    pub filter_name: *const c_char,
    pub filter_exts: *const c_char,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn build_fd(opts: *const FlexFileDialog) -> FileDialog {
    let mut fd = FileDialog::new();
    if opts.is_null() {
        return fd;
    }
    unsafe {
        let o = &*opts;
        if let Some(t) = cstr(o.title) {
            fd = fd.title(t);
        }
        if let Some(d) = cstr(o.default_dir) {
            fd = fd.default_dir(d);
        }
        if let Some(n) = cstr(o.default_name) {
            fd = fd.default_name(n);
        }
        if let Some(exts) = cstr(o.filter_exts) {
            let e: Vec<&str> = exts
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if !e.is_empty() {
                fd = fd.filter(cstr(o.filter_name).unwrap_or("文件"), &e);
            }
        }
    }
    fd
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn dialog_out(
    kind: DialogKind,
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    catch_unwind(AssertUnwindSafe(|| {
        let fd = build_fd(opts);
        match backend::show_dialog(kind, &fd) {
            Some(p) => write_str(&p.to_string_lossy(), out, out_len),
            None => -1,
        }
    }))
    .unwrap_or(-1)
}

/// 打开文件对话框，路径写入 out；返回长度，取消/出错 -1。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn flex_dialog_open_file(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::OpenFile, opts, out, out_len)
}

/// 打开目录对话框。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn flex_dialog_open_directory(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::OpenDirectory, opts, out, out_len)
}

/// 保存文件对话框。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn flex_dialog_save_file(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::SaveFile, opts, out, out_len)
}

/// 保存到目录对话框。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn flex_dialog_save_directory(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::SaveDirectory, opts, out, out_len)
}

/// 用 XML 描述启动应用并进入主事件循环（阻塞）。0 成功，负数见错误码
/// （-1 参数错、-2 XML 失败、-3 panic、-100 该平台无后端）。
///
/// 会为所有「有 name 的按钮」自动挂接点击回调（转发到 flex_set_click_callback 注册的函数）。
/// 内部按平台 cfg 二选一，保证 C 头文件里只有一份声明。
#[no_mangle]
pub extern "C" fn flex_run_xml(
    title: *const c_char,
    width: c_int,
    height: c_int,
    xml: *const c_char,
) -> c_int {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
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
                        b.on_click = Some(Box::new(move |_ctx: &mut flexui_core::EventCtx| {
                            fire_click(&name)
                        }));
                    }
                }
            });

            let mut disp = flexui_core::Dispatcher::new();
            for (group, tabbox) in res.bindings {
                disp.bind_tab(group, tabbox);
            }
            backend::run(
                flexui_core::WindowConfig::new(title, width as f32, height as f32),
                root,
                disp,
                Box::new(flexui_core::NoopDelegate),
            );
            0
        }));
        result.unwrap_or(-3)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (title, width, height, xml);
        -100 // 该平台后端未实现
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui_core::{Dispatcher, Label, Widget, WindowHandle};
    use std::sync::{atomic::AtomicUsize, atomic::Ordering, Mutex};

    struct TestWindow;

    impl WindowHandle for TestWindow {
        fn set_title(&mut self, _title: &str) {}
        fn close(&mut self) {}
        fn minimize(&mut self) {}
        fn maximize(&mut self) {}
        fn restore(&mut self) {}
    }

    #[test]
    fn ffi版本号由包版本自动生成() {
        let mut parts = env!("CARGO_PKG_VERSION").split('.');
        let major = parts.next().unwrap().parse::<u32>().unwrap();
        let minor = parts.next().unwrap().parse::<u32>().unwrap();
        let patch = parts.next().unwrap().parse::<u32>().unwrap();
        assert_eq!(flex_version(), major * 10_000 + minor * 100 + patch);
    }

    extern "C" fn ui_task(ctx: *mut c_void, user: *mut c_void) {
        const NAME: &[u8] = b"status\0";
        const TEXT: &[u8] = b"loaded\0";
        let calls = unsafe { &*(user as *const AtomicUsize) };
        calls.fetch_add(1, Ordering::SeqCst);
        flex_ctx_set_text(ctx, NAME.as_ptr().cast(), TEXT.as_ptr().cast());
    }

    #[test]
    fn ffi代理可从工作线程投递并修改控件() {
        let disp = Dispatcher::new();
        let mut root = Label::new("waiting").name("status");
        let mut window = TestWindow;
        let mut ctx = WindowCtx::with_proxy(&mut root, &mut window, disp.proxy());
        let proxy = flex_ctx_main_proxy(&mut ctx as *mut _ as *mut c_void);
        assert!(!proxy.is_null());
        drop(ctx);

        let calls = AtomicUsize::new(0);
        let proxy_addr = proxy as usize;
        let calls_addr = &calls as *const AtomicUsize as usize;
        let accepted = std::thread::spawn(move || {
            flex_main_proxy_post(
                proxy_addr as *const FlexMainProxy,
                Some(ui_task),
                calls_addr as *mut c_void,
            )
        })
        .join()
        .unwrap();
        assert_eq!(accepted, 1);

        let mut ctx = WindowCtx::new(&mut root, &mut window);
        for task in disp.drain_ui_tasks() {
            task(&mut ctx);
        }
        drop(ctx);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(root.base().text, "loaded");
        flex_main_proxy_free(proxy);
    }

    fn record(user: *mut c_void, value: c_int) {
        let calls = unsafe { &*(user as *const Mutex<Vec<c_int>>) };
        calls.lock().unwrap().push(value);
    }

    extern "C" fn before(_ctx: *mut c_void, user: *mut c_void) {
        record(user, 1);
    }

    extern "C" fn init(_ctx: *mut c_void, user: *mut c_void) {
        record(user, 2);
    }

    extern "C" fn initialized(_ctx: *mut c_void, user: *mut c_void) {
        record(user, 3);
    }

    extern "C" fn window_state(state: c_int, _ctx: *mut c_void, user: *mut c_void) {
        record(user, state + 10);
    }

    extern "C" fn closing(_ctx: *mut c_void, user: *mut c_void) -> c_int {
        record(user, 20);
        0
    }

    extern "C" fn closed(user: *mut c_void) {
        record(user, 21);
    }

    #[test]
    fn ffi桥接生命周期和窗口状态() {
        let calls = Mutex::new(Vec::new());
        let mut delegate = CDelegate {
            d: FlexDelegate {
                on_before_init: Some(before),
                on_init: Some(init),
                on_initialized: Some(initialized),
                on_click: None,
                on_control_event: None,
                on_double_click: None,
                on_context: None,
                on_size: None,
                on_key: None,
                on_window_state: Some(window_state),
                on_closing: Some(closing),
                on_closed: Some(closed),
            },
            user: &calls as *const Mutex<Vec<c_int>> as usize,
        };
        let mut root = Label::new("test");
        let mut window = TestWindow;
        let mut ctx = WindowCtx::new(&mut root, &mut window);

        delegate.on_before_init(&mut ctx);
        delegate.on_init(&mut ctx);
        delegate.on_initialized(&mut ctx);
        delegate.on_window_event(&WindowEvent::Maximized, &mut ctx);
        assert!(!delegate.on_closing(&mut ctx));
        delegate.on_closed();

        assert_eq!(
            *calls.lock().unwrap(),
            vec![1, 2, 3, FLEX_WINDOW_MAXIMIZED + 10, 20, 21]
        );
    }

    /// 记录 on_control_event 摊平后的 (ev_type, i0, f0, 文本) 供断言。
    static CTRL_LOG: Mutex<Vec<(c_int, c_int, f32, String)>> = Mutex::new(Vec::new());

    extern "C" fn control_event(
        _name: *const c_char,
        ev_type: c_int,
        i0: c_int,
        f0: f32,
        _f1: f32,
        text: *const c_char,
        _ctx: *mut c_void,
        _user: *mut c_void,
    ) {
        let text = unsafe { cstr(text) }.unwrap_or("").to_string();
        CTRL_LOG.lock().unwrap().push((ev_type, i0, f0, text));
    }

    #[test]
    fn ffi控件事件摊平映射() {
        CTRL_LOG.lock().unwrap().clear();
        let mut delegate = CDelegate {
            d: FlexDelegate {
                on_before_init: None,
                on_init: None,
                on_initialized: None,
                on_click: None,
                on_control_event: Some(control_event),
                on_double_click: None,
                on_context: None,
                on_size: None,
                on_key: None,
                on_window_state: None,
                on_closing: None,
                on_closed: None,
            },
            user: std::ptr::null_mut::<c_void>() as usize,
        };
        let mut root = Label::new("test");
        let mut window = TestWindow;
        let mut ctx = WindowCtx::new(&mut root, &mut window);

        delegate.on_control_event("chk", &ControlEvent::SelectedChanged(true), &mut ctx);
        delegate.on_control_event("sld", &ControlEvent::ValueChanged(0.5), &mut ctx);
        delegate.on_control_event("ed", &ControlEvent::TextChanged("hi".into()), &mut ctx);
        delegate.on_control_event("cbo", &ControlEvent::SelectionChanged(None), &mut ctx);
        // 虚拟列表事件不导出到 C，不应回调。
        delegate.on_control_event("vl", &ControlEvent::RowsSelectionChanged(vec![1]), &mut ctx);

        let log = CTRL_LOG.lock().unwrap();
        assert_eq!(log.len(), 4, "只应回调 4 个已导出的事件");
        assert_eq!((log[0].0, log[0].1), (FLEX_CTRL_SELECTED_CHANGED, 1));
        assert_eq!((log[1].0, log[1].2), (FLEX_CTRL_VALUE_CHANGED, 0.5));
        assert_eq!((log[2].0, log[2].3.as_str()), (FLEX_CTRL_TEXT_CHANGED, "hi"));
        assert_eq!((log[3].0, log[3].1), (FLEX_CTRL_SELECTION_CHANGED, -1));
    }
}
