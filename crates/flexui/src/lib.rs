//! flexui：面向用户的门面 crate（L5）。
//!
//! 面向对象用法（参考 duilib WinImplBase/Window）：实现 `WindowImpl` trait（≈ WindowImplBase），
//! 用 `Window::new(MyWindow).run()` 启动（≈ Window）。控件层用组合式 + 能力 trait 表达层级。

use std::sync::{OnceLock, RwLock};

// 核心：控件、状态、样式、布局、事件、分发、窗口抽象。
pub use flexui_core::*;

// XML 布局加载。
pub use flexui_xml::{
    build_fragment_res, build_fragment_str, load_native_menu_res, load_native_menu_str, load_res,
    load_str as load_xml_str, load_window_res, load_window_str, Context, LoadError, LoadResult,
    WindowDoc,
};

// 资源系统（RM1-5）。
pub use flexui_i18n::{
    I18nError, LocalizationValue, LocalizedStringKey, LocalizedStringResource, Localizer,
};
pub use flexui_resource::{DirProvider, ResError, ResourceManager, ResourceProvider, ZipProvider};

fn application_localizer_slot() -> &'static RwLock<Option<Localizer>> {
    static SLOT: OnceLock<RwLock<Option<Localizer>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(None))
}

fn application_theme_slot() -> &'static RwLock<Theme> {
    static SLOT: OnceLock<RwLock<Theme>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(Theme::light()))
}

/// 设置应用级主题；后续创建的窗口自动继承。
pub fn set_application_theme(theme: Theme) {
    *application_theme_slot().write().unwrap() = theme;
}

pub fn application_theme() -> Theme {
    application_theme_slot().read().unwrap().clone()
}

/// 设置 SwiftUI Environment 风格的应用级本地化环境；后续创建的窗口自动继承。
pub fn set_application_localizer(localizer: Localizer) {
    *application_localizer_slot().write().unwrap() = Some(localizer);
}

pub fn application_localizer() -> Option<Localizer> {
    application_localizer_slot().read().unwrap().clone()
}

// —— 平台后端选择（仅内部使用，不再暴露自由函数 run/run_xml）——
#[cfg(target_os = "macos")]
use flexui_macos::run_multi as backend_run_multi;
#[cfg(target_os = "windows")]
use flexui_windows::run_multi as backend_run_multi;
#[cfg(target_os = "linux")]
use flexui_linux::run_multi as backend_run_multi;

/// 设置应用图标。macOS 用于 Dock/应用切换器；Windows 的 EXE 图标由构建资源提供。
pub fn set_application_icon(bytes: &[u8]) {
    #[cfg(target_os = "macos")]
    flexui_macos::set_application_icon(bytes);
    #[cfg(target_os = "linux")]
    flexui_linux::set_application_icon(bytes);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = bytes;
}

/// 系统剪贴板文本读写（macOS NSPasteboard / Windows CF_UNICODETEXT）。
pub mod clipboard {
    /// 读取系统剪贴板文本（无则 None）。
    pub fn get_text() -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            flexui_macos::clipboard_get_text()
        }
        #[cfg(target_os = "windows")]
        {
            flexui_windows::clipboard_get_text()
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            None
        }
    }

    /// 写入系统剪贴板文本。
    #[allow(unused_variables)]
    pub fn set_text(text: &str) {
        #[cfg(target_os = "macos")]
        {
            flexui_macos::clipboard_set_text(text);
        }
        #[cfg(target_os = "windows")]
        {
            flexui_windows::clipboard_set_text(text);
        }
    }
}

/// 系统文件对话框（封装各平台原生弹窗：macOS NSOpenPanel/NSSavePanel、
/// Windows GetOpenFileName/SHBrowseForFolder）。支持标题、默认路径、扩展名筛选。
///
/// 用法（在窗口回调里调用，模态阻塞直到用户选择/取消）：
/// ```ignore
/// let f = flexui::dialog::open_file(&FileDialog::new().title("选图片").filter("图片", &["png","jpg"]));
/// ```
pub mod dialog {
    use std::path::PathBuf;

    pub use flexui_core::dialog::{DialogKind, FileDialog, FileFilter};

    #[allow(unused_variables)]
    fn show(kind: DialogKind, opts: &FileDialog) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            flexui_macos::show_dialog(kind, opts)
        }
        #[cfg(target_os = "windows")]
        {
            flexui_windows::show_dialog(kind, opts)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            None
        }
    }

    /// 打开文件。
    pub fn open_file(opts: &FileDialog) -> Option<PathBuf> {
        show(DialogKind::OpenFile, opts)
    }
    /// 打开目录。
    pub fn open_directory(opts: &FileDialog) -> Option<PathBuf> {
        show(DialogKind::OpenDirectory, opts)
    }
    /// 保存文件。
    pub fn save_file(opts: &FileDialog) -> Option<PathBuf> {
        show(DialogKind::SaveFile, opts)
    }
    /// 保存到目录（选择一个目录）。
    pub fn save_directory(opts: &FileDialog) -> Option<PathBuf> {
        show(DialogKind::SaveDirectory, opts)
    }
}

/// 皮肤来源：XML 字符串、资源逻辑路径、或代码构建的控件树。
pub enum Skin {
    /// XML 字符串（图片按文件路径解析）。
    Xml(String),
    /// 资源逻辑路径（XML 与图片都经 `WindowImpl::resources()` 的 ResourceManager 解析，支持 zip/内嵌）。
    Res(String),
    /// 直接给定控件树（代码构建）。
    Tree(Node),
}

impl Skin {
    pub fn xml(s: impl Into<String>) -> Self {
        Skin::Xml(s.into())
    }
    pub fn res(path: impl Into<String>) -> Self {
        Skin::Res(path.into())
    }
    pub fn tree(node: Node) -> Self {
        Skin::Tree(node)
    }
}

/// 用户「继承」的窗口基类（≈ duilib WindowImplBase）。
///
/// 必需：`skin`（界面来源）。`config` 是 XML 不含 `<Window>` 时使用的兜底配置。
pub trait WindowImpl: 'static {
    /// 窗口配置（标题/尺寸/resizable/标题栏）。
    fn config(&self) -> WindowConfig {
        WindowConfig::default()
    }
    /// 界面来源（XML / 资源路径 / 控件树）。
    fn skin(&self) -> Skin;

    /// 资源管理器（用于 Skin::Res 及图片解析）。默认空；可挂 Dir/Zip/内嵌 provider。
    fn resources(&self) -> ResourceManager {
        ResourceManager::new()
    }

    /// SwiftUI Environment 风格的共享本地化环境；None 表示全部文本按字面量处理。
    fn localizer(&self) -> Option<Localizer> {
        application_localizer()
    }

    /// 平台窗口和控件树已建立、应用初始化逻辑尚未执行。
    fn on_before_init(&mut self, _ctx: &mut WindowCtx) {}
    /// 窗口初始化（≈ InitWindow）：绑事件、预设文本或取得 `MainProxy`。
    fn on_init(&mut self, _ctx: &mut WindowCtx) {}
    /// `on_init` 已执行完成，窗口即将显示首帧。
    fn on_initialized(&mut self, _ctx: &mut WindowCtx) {}
    /// 某具名控件被点击（≈ Notify）。
    fn on_click(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 具名控件的 hover、focus、文本、选择和值等语义变化。
    fn on_control_event(&mut self, _name: &str, _event: &ControlEvent, _ctx: &mut WindowCtx) {}
    /// 窗口焦点、尺寸、DPI、键盘及窗口状态等统一事件。
    fn on_window_event(&mut self, _event: &WindowEvent, _ctx: &mut WindowCtx) {}
    /// 某具名控件被双击。
    fn on_double_click(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 某具名控件被右键（上下文菜单），坐标为逻辑像素。
    fn on_context(&mut self, _name: &str, _x: f32, _y: f32, _ctx: &mut WindowCtx) {}
    /// 窗口尺寸变化。
    fn on_size(&mut self, _width: f32, _height: f32, _ctx: &mut WindowCtx) {}
    /// 键盘按下。
    fn on_key(&mut self, _key: u32, _ctx: &mut WindowCtx) {}
    /// 鼠标滚轮（dx/dy 为滚动增量）。滚动容器已先行处理；窗口层可据此做缩放等。
    fn on_wheel(&mut self, _dx: f32, _dy: f32, _ctx: &mut WindowCtx) {}
    /// 后台线程经 `MainProxy` 投递的消息（主线程处理）。
    fn on_message(&mut self, _msg: &str, _ctx: &mut WindowCtx) {}
    /// 文件拖放到窗口（绝对路径）。
    fn on_drop_files(&mut self, _paths: &[String], _ctx: &mut WindowCtx) {}
    /// 即将关闭；返回 false 阻止关闭。默认转发旧版 `on_close`。
    fn on_closing(&mut self, ctx: &mut WindowCtx) -> bool {
        self.on_close(ctx)
    }
    /// 兼容旧版关闭钩子；新代码优先实现 `on_closing`。
    fn on_close(&mut self, _ctx: &mut WindowCtx) -> bool {
        true
    }
    /// 原生窗口已经关闭；此时不能再访问 `WindowCtx`。
    fn on_closed(&mut self) {}
}

/// 把 WindowImpl 适配成后端可调用的 WindowDelegate。
struct ImplDelegate<W: WindowImpl> {
    imp: W,
}

impl<W: WindowImpl> WindowDelegate for ImplDelegate<W> {
    fn on_before_init(&mut self, ctx: &mut WindowCtx) {
        self.imp.on_before_init(ctx);
    }
    fn on_init(&mut self, ctx: &mut WindowCtx) {
        self.imp.on_init(ctx);
    }
    fn on_initialized(&mut self, ctx: &mut WindowCtx) {
        self.imp.on_initialized(ctx);
    }
    fn on_activate(&mut self, name: &str, ctx: &mut WindowCtx) {
        self.imp.on_click(name, ctx);
    }
    fn on_control_event(&mut self, name: &str, event: &ControlEvent, ctx: &mut WindowCtx) {
        self.imp.on_control_event(name, event, ctx);
    }
    fn on_window_event(&mut self, event: &WindowEvent, ctx: &mut WindowCtx) {
        self.imp.on_window_event(event, ctx);
    }
    fn on_double_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        self.imp.on_double_click(name, ctx);
    }
    fn on_context(&mut self, name: &str, x: f32, y: f32, ctx: &mut WindowCtx) {
        self.imp.on_context(name, x, y, ctx);
    }
    fn on_size(&mut self, w: f32, h: f32, ctx: &mut WindowCtx) {
        self.imp.on_size(w, h, ctx);
    }
    fn on_key(&mut self, key: u32, ctx: &mut WindowCtx) {
        self.imp.on_key(key, ctx);
    }
    fn on_wheel(&mut self, dx: f32, dy: f32, ctx: &mut WindowCtx) {
        self.imp.on_wheel(dx, dy, ctx);
    }
    fn on_message(&mut self, msg: &str, ctx: &mut WindowCtx) {
        self.imp.on_message(msg, ctx);
    }
    fn on_drop_files(&mut self, paths: &[String], ctx: &mut WindowCtx) {
        self.imp.on_drop_files(paths, ctx);
    }
    fn on_closing(&mut self, ctx: &mut WindowCtx) -> bool {
        self.imp.on_closing(ctx)
    }
    fn on_close(&mut self, ctx: &mut WindowCtx) -> bool {
        self.imp.on_close(ctx)
    }
    fn on_closed(&mut self) {
        self.imp.on_closed();
    }
}

/// 把一个 `WindowImpl` 加载成可交给后端创建的窗口规格。
///
/// 可用于多窗口：在任意窗口回调里用 `build_window` 构造规格后传给 `ctx.open_window`。
pub fn build_window<W: WindowImpl>(imp: W) -> Result<NewWindow, LoadError> {
    let localizer = imp.localizer();
    let mut ctx = Context::new();
    if let Some(value) = localizer.clone() {
        ctx.set_localizer(value);
    }
    // 皮肤 XML 若以 <Window> 为根，则其属性提供窗口配置（W6），否则用 imp.config()。
    let (config, mut root, bindings) = match imp.skin() {
        Skin::Xml(xml) => {
            let doc = load_window_str(&xml, &ctx)?;
            (
                doc.config.unwrap_or_else(|| imp.config()),
                doc.root,
                doc.bindings,
            )
        }
        Skin::Res(path) => {
            let res = imp.resources();
            let doc = load_window_res(&res, &path, &ctx)?;
            (
                doc.config.unwrap_or_else(|| imp.config()),
                doc.root,
                doc.bindings,
            )
        }
        Skin::Tree(node) => (imp.config(), node, Vec::new()),
    };
    if let Some(localizer) = localizer.as_ref() {
        apply_localizations(&mut root, localizer);
    }
    apply_theme(root.as_mut(), &application_theme());
    let mut disp = Dispatcher::new();
    for (group, tabbox) in bindings {
        disp.bind_tab(group, tabbox);
    }
    Ok(NewWindow {
        config,
        root,
        disp,
        delegate: Box::new(ImplDelegate { imp }),
        presentation: WindowPresentation::Normal,
        locale_revision: localizer.as_ref().map_or(0, Localizer::revision),
        localizer,
    })
}

/// 启动多个窗口，共享同一个平台事件循环。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub fn run_multi(windows: Vec<NewWindow>) {
    backend_run_multi(windows);
}

/// 窗口驱动（≈ duilib Window）：加载皮肤、建原生窗口、进事件循环。用户不继承，直接用。
pub struct Window<W: WindowImpl> {
    imp: W,
}

impl<W: WindowImpl> Window<W> {
    /// 用一个 WindowImpl 创建窗口驱动。
    pub fn new(imp: W) -> Self {
        Self { imp }
    }

    /// 居中显示（≈ CenterWindow；当前后端默认即居中，保留以对齐习惯用法）。
    pub fn center(self) -> Self {
        self
    }

    /// 启动：加载皮肤 → 建窗 → 初始化生命周期 → 进主事件循环（阻塞）。
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    pub fn run(self) {
        let spec = match build_window(self.imp) {
            Ok(spec) => spec,
            Err(e) => {
                eprintln!("[flexui] 窗口加载失败: {e}");
                return;
            }
        };
        // 保留完整 NewWindow，确保主窗口与后续窗口共享本地化环境及其修订号。
        backend_run_multi(vec![spec]);
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct TestWindowHandle;
    impl WindowHandle for TestWindowHandle {
        fn set_title(&mut self, _title: &str) {}
        fn close(&mut self) {}
        fn minimize(&mut self) {}
        fn maximize(&mut self) {}
        fn restore(&mut self) {}
    }

    struct LifecycleWindow {
        calls: Rc<RefCell<Vec<&'static str>>>,
    }

    impl WindowImpl for LifecycleWindow {
        fn skin(&self) -> Skin {
            Skin::tree(Box::new(Label::new("test")))
        }

        fn on_before_init(&mut self, _ctx: &mut WindowCtx) {
            self.calls.borrow_mut().push("before_init");
        }

        fn on_init(&mut self, _ctx: &mut WindowCtx) {
            self.calls.borrow_mut().push("init");
        }

        fn on_initialized(&mut self, _ctx: &mut WindowCtx) {
            self.calls.borrow_mut().push("initialized");
        }

        fn on_close(&mut self, _ctx: &mut WindowCtx) -> bool {
            self.calls.borrow_mut().push("close_compat");
            false
        }

        fn on_closed(&mut self) {
            self.calls.borrow_mut().push("closed");
        }
    }

    #[test]
    fn 生命周期适配顺序完整且兼容旧关闭钩子() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut delegate = ImplDelegate {
            imp: LifecycleWindow {
                calls: calls.clone(),
            },
        };
        let mut root = Label::new("test");
        let mut window = TestWindowHandle;
        let mut ctx = WindowCtx::new(&mut root, &mut window);

        delegate.on_before_init(&mut ctx);
        delegate.on_init(&mut ctx);
        delegate.on_initialized(&mut ctx);
        assert!(!delegate.on_closing(&mut ctx));
        delegate.on_closed();

        assert_eq!(
            *calls.borrow(),
            vec![
                "before_init",
                "init",
                "initialized",
                "close_compat",
                "closed"
            ]
        );
    }
}
