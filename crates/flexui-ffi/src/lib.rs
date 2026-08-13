//! flexui-ffi：C ABI 导出（L6，预留 R6）。
//!
//! 采用「不透明句柄 + 函数 + 回调指针」形态，所有入口用 `catch_unwind` 防止
//! panic 穿越 C 边界。一期提供最小可用集：版本、XML 校验、点击回调、运行。

// C ABI 入口按惯例暴露安全签名（裸指针参数在内部经空指针检查后解引用）。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use flexui_core::{visit_all_mut, WidgetRole, WindowCtx, WindowDelegate};
use flexui_xml::{load_str, Context};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use flexui_core::dialog::{DialogKind, FileDialog};

// 按平台选择后端（编译期二选一）。
#[cfg(target_os = "macos")]
use flexui_macos as backend;
#[cfg(target_os = "windows")]
use flexui_windows as backend;

/// 库版本（主*10000 + 次*100 + 补丁）。
#[no_mangle]
pub extern "C" fn flex_version() -> u32 {
    1 // 0.0.1 → 简单返回 1
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

// —— 窗口委托桥接：C 侧函数指针集 ——

/// C 侧窗口委托：各钩子为可空函数指针，第一个参数为不透明 FlexCtx*。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlexDelegate {
    pub on_init: Option<extern "C" fn(ctx: *mut c_void, user: *mut c_void)>,
    pub on_click: Option<extern "C" fn(name: *const c_char, ctx: *mut c_void, user: *mut c_void)>,
    pub on_context: Option<
        extern "C" fn(name: *const c_char, x: f32, y: f32, ctx: *mut c_void, user: *mut c_void),
    >,
    /// 返回非 0 允许关闭，0 阻止关闭。
    pub on_close: Option<extern "C" fn(ctx: *mut c_void, user: *mut c_void) -> c_int>,
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
    fn on_init(&mut self, ctx: &mut WindowCtx) {
        if let Some(f) = self.d.on_init {
            f(ctx as *mut _ as *mut c_void, self.user_ptr());
        }
    }
    fn on_activate(&mut self, name: &str, ctx: &mut WindowCtx) {
        if let Some(f) = self.d.on_click {
            if let Ok(cn) = CString::new(name) {
                f(cn.as_ptr(), ctx as *mut _ as *mut c_void, self.user_ptr());
            }
        }
    }
    fn on_context(&mut self, name: &str, x: f32, y: f32, ctx: &mut WindowCtx) {
        if let Some(f) = self.d.on_context {
            if let Ok(cn) = CString::new(name) {
                f(
                    cn.as_ptr(),
                    x,
                    y,
                    ctx as *mut _ as *mut c_void,
                    self.user_ptr(),
                );
            }
        }
    }
    fn on_close(&mut self, ctx: &mut WindowCtx) -> bool {
        match self.d.on_close {
            Some(f) => f(ctx as *mut _ as *mut c_void, self.user_ptr()) != 0,
            None => true,
        }
    }
}

/// 用 XML + C 委托启动应用（阻塞）。delegate 为 NULL 时等价于 flex_run_xml。
/// 0 成功，负数错误码（-1 参数错、-2 XML 失败、-3 panic、-100 无后端）。
#[cfg(any(target_os = "macos", target_os = "windows"))]
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
        let (Some(title), Some(xml)) = (unsafe { cstr(title) }, unsafe { cstr(xml) }) else {
            return -1;
        };
        let ctx = Context::new();
        let res = match load_str(xml, &ctx) {
            Ok(r) => r,
            Err(_) => return -2,
        };
        let mut disp = flexui_core::Dispatcher::new();
        for (group, tabbox) in res.bindings {
            disp.bind_tab(group, tabbox);
        }
        let deleg: Box<dyn WindowDelegate> = if delegate.is_null() {
            Box::new(flexui_core::NoopDelegate)
        } else {
            Box::new(CDelegate {
                d: unsafe { *delegate },
                user: user as usize,
            })
        };
        backend::run(
            flexui_core::WindowConfig::new(title, width as f32, height as f32),
            res.root,
            disp,
            deleg,
        );
        0
    }));
    result.unwrap_or(-3)
}

// —— 系统剪贴板 ——

/// 读系统剪贴板文本到 out（含 NUL）；返回长度，空/出错返回 -1。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[no_mangle]
pub extern "C" fn flex_clipboard_get_text(out: *mut c_char, out_len: c_int) -> c_int {
    catch_unwind(AssertUnwindSafe(|| match backend::clipboard_get_text() {
        Some(s) => write_str(&s, out, out_len),
        None => -1,
    }))
    .unwrap_or(-1)
}

/// 写系统剪贴板文本。
#[cfg(any(target_os = "macos", target_os = "windows"))]
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
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
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[no_mangle]
pub extern "C" fn flex_dialog_open_file(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::OpenFile, opts, out, out_len)
}

/// 打开目录对话框。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[no_mangle]
pub extern "C" fn flex_dialog_open_directory(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::OpenDirectory, opts, out, out_len)
}

/// 保存文件对话框。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[no_mangle]
pub extern "C" fn flex_dialog_save_file(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::SaveFile, opts, out, out_len)
}

/// 保存到目录对话框。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[no_mangle]
pub extern "C" fn flex_dialog_save_directory(
    opts: *const FlexFileDialog,
    out: *mut c_char,
    out_len: c_int,
) -> c_int {
    dialog_out(DialogKind::SaveDirectory, opts, out, out_len)
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
