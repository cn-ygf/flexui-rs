//! flexui：面向用户的门面 crate（L5）。
//!
//! 面向对象用法（参考 duilib WinImplBase/Window）：实现 `WindowImpl` trait（≈ WindowImplBase），
//! 用 `Window::new(MyWindow).run()` 启动（≈ Window）。控件层用组合式 + 能力 trait 表达层级。

// 核心：控件、状态、样式、布局、事件、分发、窗口抽象。
pub use flexui_core::*;

// XML 布局加载。
pub use flexui_xml::{
    build_fragment_res, build_fragment_str, load_res, load_str as load_xml_str, load_window_res,
    load_window_str, Context, LoadError, LoadResult, WindowDoc,
};

// 资源系统（RM1-5）。
pub use flexui_resource::{DirProvider, ResError, ResourceManager, ResourceProvider, ZipProvider};

// —— 平台后端选择（仅内部使用，不再暴露自由函数 run/run_xml）——
#[cfg(target_os = "macos")]
use flexui_macos::run as backend_run;
#[cfg(target_os = "macos")]
use flexui_macos::run_multi as backend_run_multi;
#[cfg(target_os = "windows")]
use flexui_windows::run as backend_run;
#[cfg(target_os = "windows")]
use flexui_windows::run_multi as backend_run_multi;

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

    /// 窗口与控件创建完成（≈ InitWindow）：绑事件、预设文本等。
    fn on_init(&mut self, _ctx: &mut WindowCtx) {}
    /// 某具名控件被点击（≈ Notify）。
    fn on_click(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 某具名控件被双击。
    fn on_double_click(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 某具名控件被右键（上下文菜单），坐标为逻辑像素。
    fn on_context(&mut self, _name: &str, _x: f32, _y: f32, _ctx: &mut WindowCtx) {}
    /// 窗口尺寸变化。
    fn on_size(&mut self, _width: f32, _height: f32, _ctx: &mut WindowCtx) {}
    /// 键盘按下。
    fn on_key(&mut self, _key: u32, _ctx: &mut WindowCtx) {}
    /// 后台线程经 `MainProxy` 投递的消息（主线程处理）。
    fn on_message(&mut self, _msg: &str, _ctx: &mut WindowCtx) {}
    /// 文件拖放到窗口（绝对路径）。
    fn on_drop_files(&mut self, _paths: &[String], _ctx: &mut WindowCtx) {}
    /// 关闭请求；返回 false 阻止关闭。
    fn on_close(&mut self, _ctx: &mut WindowCtx) -> bool {
        true
    }
}

/// 把 WindowImpl 适配成后端可调用的 WindowDelegate。
struct ImplDelegate<W: WindowImpl> {
    imp: W,
}

impl<W: WindowImpl> WindowDelegate for ImplDelegate<W> {
    fn on_init(&mut self, ctx: &mut WindowCtx) {
        self.imp.on_init(ctx);
    }
    fn on_activate(&mut self, name: &str, ctx: &mut WindowCtx) {
        self.imp.on_click(name, ctx);
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
    fn on_message(&mut self, msg: &str, ctx: &mut WindowCtx) {
        self.imp.on_message(msg, ctx);
    }
    fn on_drop_files(&mut self, paths: &[String], ctx: &mut WindowCtx) {
        self.imp.on_drop_files(paths, ctx);
    }
    fn on_close(&mut self, ctx: &mut WindowCtx) -> bool {
        self.imp.on_close(ctx)
    }
}

/// 把一个 `WindowImpl` 加载成可交给后端创建的窗口规格。
///
/// 可用于多窗口：在任意窗口回调里用 `build_window` 构造规格后传给 `ctx.open_window`。
pub fn build_window<W: WindowImpl>(imp: W) -> Result<NewWindow, LoadError> {
    let ctx = Context::new();
    // 皮肤 XML 若以 <Window> 为根，则其属性提供窗口配置（W6），否则用 imp.config()。
    let (config, root, bindings) = match imp.skin() {
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
    let mut disp = Dispatcher::new();
    for (group, tabbox) in bindings {
        disp.bind_tab(group, tabbox);
    }
    Ok(NewWindow {
        config,
        root,
        disp,
        delegate: Box::new(ImplDelegate { imp }),
    })
}

/// 启动多个窗口，共享同一个平台事件循环。
#[cfg(any(target_os = "macos", target_os = "windows"))]
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

    /// 启动：加载皮肤 → 建窗 → on_init → 进主事件循环（阻塞）。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn run(self) {
        let spec = match build_window(self.imp) {
            Ok(spec) => spec,
            Err(e) => {
                eprintln!("[flexui] 窗口加载失败: {e}");
                return;
            }
        };
        backend_run(spec.config, spec.root, spec.disp, spec.delegate);
    }
}
