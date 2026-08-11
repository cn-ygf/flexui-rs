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
pub use flexui_resource::{
    DirProvider, ResError, ResourceManager, ResourceProvider, ZipProvider,
};

// —— 平台后端选择（仅内部使用，不再暴露自由函数 run/run_xml）——
#[cfg(target_os = "macos")]
use flexui_macos::run as backend_run;
#[cfg(target_os = "windows")]
use flexui_windows::run as backend_run;

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
/// 必需：`config`（窗口配置）、`skin`（界面来源）。其余为可重写钩子，默认空实现。
pub trait WindowImpl: 'static {
    /// 窗口配置（标题/尺寸/resizable/标题栏）。
    fn config(&self) -> WindowConfig;
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
    fn on_close(&mut self, ctx: &mut WindowCtx) -> bool {
        self.imp.on_close(ctx)
    }
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
        let ctx = Context::new();
        // 皮肤 XML 若以 <Window> 为根，则其属性提供窗口配置（W6），否则用 imp.config()。
        let (config, root, bindings) = match self.imp.skin() {
            Skin::Xml(xml) => match load_window_str(&xml, &ctx) {
                Ok(doc) => (
                    doc.config.unwrap_or_else(|| self.imp.config()),
                    doc.root,
                    doc.bindings,
                ),
                Err(e) => {
                    eprintln!("[flexui] 皮肤 XML 加载失败: {e}");
                    return;
                }
            },
            Skin::Res(path) => {
                let res = self.imp.resources();
                match load_window_res(&res, &path, &ctx) {
                    Ok(doc) => (
                        doc.config.unwrap_or_else(|| self.imp.config()),
                        doc.root,
                        doc.bindings,
                    ),
                    Err(e) => {
                        eprintln!("[flexui] 皮肤资源加载失败: {e}");
                        return;
                    }
                }
            }
            Skin::Tree(node) => (self.imp.config(), node, Vec::new()),
        };
        let mut disp = Dispatcher::new();
        for (group, tabbox) in bindings {
            disp.bind_tab(group, tabbox);
        }
        backend_run(config, root, disp, Box::new(ImplDelegate { imp: self.imp }));
    }
}
