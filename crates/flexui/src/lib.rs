//! flexui：面向用户的门面 crate（L5）。
//!
//! 面向对象用法（参考 duilib WinImplBase/Window）：实现 `WindowImpl` trait（≈ WindowImplBase），
//! 用 `Window::new(MyWindow).run()` 启动（≈ Window）。控件层用组合式 + 能力 trait 表达层级。

// 核心：控件、状态、样式、布局、事件、分发、窗口抽象。
pub use flexui_core::*;

// XML 布局加载。
pub use flexui_xml::{load_str as load_xml_str, Context, LoadError, LoadResult};

// —— 平台后端选择（仅内部使用，不再暴露自由函数 run/run_xml）——
#[cfg(target_os = "macos")]
use flexui_macos::run as backend_run;
#[cfg(target_os = "windows")]
use flexui_windows::run as backend_run;

/// 皮肤来源：XML 描述或代码构建的控件树。
pub enum Skin {
    /// XML 字符串（驱动加载并 build，含 tabbar 绑定）。
    Xml(String),
    /// 直接给定控件树（代码构建）。
    Tree(Node),
}

impl Skin {
    pub fn xml(s: impl Into<String>) -> Self {
        Skin::Xml(s.into())
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
    /// 界面来源（XML 或控件树）。
    fn skin(&self) -> Skin;

    /// 窗口与控件创建完成（≈ InitWindow）：绑事件、预设文本等。
    fn on_init(&mut self, _ctx: &mut WindowCtx) {}
    /// 某具名控件被点击（≈ Notify）。
    fn on_click(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 窗口尺寸变化。
    fn on_size(&mut self, _width: f32, _height: f32, _ctx: &mut WindowCtx) {}
    /// 键盘按下。
    fn on_key(&mut self, _key: u32, _ctx: &mut WindowCtx) {}
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
    fn on_size(&mut self, w: f32, h: f32, ctx: &mut WindowCtx) {
        self.imp.on_size(w, h, ctx);
    }
    fn on_key(&mut self, key: u32, ctx: &mut WindowCtx) {
        self.imp.on_key(key, ctx);
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
        let config = self.imp.config();
        let (root, bindings) = match self.imp.skin() {
            Skin::Xml(xml) => {
                let ctx = Context::new();
                match load_xml_str(&xml, &ctx) {
                    Ok(r) => (r.root, r.bindings),
                    Err(e) => {
                        eprintln!("[flexui] 皮肤 XML 加载失败: {e}");
                        return;
                    }
                }
            }
            Skin::Tree(node) => (node, Vec::new()),
        };
        let mut disp = Dispatcher::new();
        for (group, tabbox) in bindings {
            disp.bind_tab(group, tabbox);
        }
        backend_run(config, root, disp, Box::new(ImplDelegate { imp: self.imp }));
    }
}
