//! 窗口 OO 抽象（L3）。平台无关的窗口配置、控制句柄、上下文与委托接口。
//!
//! 对齐 duilib：`WindowDelegate` ≈ WindowImplBase 的运行期钩子；`WindowHandle` ≈ Window
//! 的控制 API；`WindowCtx` ≈ 传给钩子、可 FindControl + 控制窗口的上下文。
//! 用户面向的 `WindowImpl`/`Window`/`Skin` 在 facade（flexui）里，本模块是其底座。

use crate::dispatch::EventCtx;
use crate::widget::Widget;

/// 标题栏模式（一期先实现 System；隐藏留控件/无边框见开发计划阶段 3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TitlebarMode {
    /// 原生标题栏 + 系统按钮。
    #[default]
    System,
    /// 隐藏系统标题栏（macOS 保留交通灯 / Windows 自绘按钮）。
    HiddenKeepControls,
    /// 完全无边框。
    None,
}

/// 窗口配置。
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: f32,
    pub height: f32,
    /// false = 禁止改变大小。
    pub resizable: bool,
    pub titlebar: TitlebarMode,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "flexui-rs".to_string(),
            width: 640.0,
            height: 440.0,
            resizable: true,
            titlebar: TitlebarMode::System,
        }
    }
}

impl WindowConfig {
    pub fn new(title: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            ..Default::default()
        }
    }
    pub fn resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }
    pub fn titlebar(mut self, m: TitlebarMode) -> Self {
        self.titlebar = m;
        self
    }
}

/// 窗口控制句柄（由后端实现）：≈ duilib Window 的 Close/ShowWindow 等。
pub trait WindowHandle {
    fn set_title(&mut self, title: &str);
    fn close(&mut self);
    fn minimize(&mut self);
    fn maximize(&mut self);
    fn restore(&mut self);
}

/// 上下文菜单等浮层的打开请求（由 WindowCtx 收集，后端排空后交分发器）。
pub struct OverlayRequest {
    /// 锚点矩形（右键点位可用 0 尺寸 rect）。
    pub anchor: flexui_geometry::Rect,
    /// 菜单项 (标签, name)；选中后按 name 经 on_activate 上报。
    pub items: Vec<(String, String)>,
}

/// 属性动画请求（由 WindowCtx 收集，后端排空后交分发器）。
pub struct AnimRequest {
    pub name: String,
    pub prop: crate::anim::AnimProp,
    pub to: f32,
    pub dur_secs: f32,
    pub easing: crate::anim::Easing,
}

/// 传给窗口钩子的上下文：既能按 name 访问控件树，也能控制窗口、弹出菜单。
/// ≈ duilib 里 InitWindow/Notify 内 `FindControl(...)` + `pWnd->Close()` 的能力。
pub struct WindowCtx<'a> {
    root: &'a mut dyn Widget,
    win: &'a mut dyn WindowHandle,
    overlay_requests: Vec<OverlayRequest>,
    anim_requests: Vec<AnimRequest>,
    proxy: Option<crate::dispatch::MainProxy>,
}

impl<'a> WindowCtx<'a> {
    pub fn new(root: &'a mut dyn Widget, win: &'a mut dyn WindowHandle) -> Self {
        Self {
            root,
            win,
            overlay_requests: Vec::new(),
            anim_requests: Vec::new(),
            proxy: None,
        }
    }

    /// 带主线程投递句柄构造（后端在 on_init 等处提供，供 app 拿去发给工作线程）。
    pub fn with_proxy(
        root: &'a mut dyn Widget,
        win: &'a mut dyn WindowHandle,
        proxy: crate::dispatch::MainProxy,
    ) -> Self {
        let mut c = Self::new(root, win);
        c.proxy = Some(proxy);
        c
    }

    /// 取主线程投递句柄（在 on_init 里获取，clone 给工作线程后 `send`）。
    pub fn main_proxy(&self) -> Option<crate::dispatch::MainProxy> {
        self.proxy.clone()
    }

    /// 在 anchor 处弹出上下文菜单；items 为 (标签, name)。选中项经 on_activate 上报。
    pub fn open_menu(&mut self, anchor: flexui_geometry::Rect, items: Vec<(String, String)>) {
        self.overlay_requests.push(OverlayRequest { anchor, items });
    }

    /// 后端取走本轮浮层请求（随后交给分发器 open_menu）。
    pub fn take_overlay_requests(&mut self) -> Vec<OverlayRequest> {
        std::mem::take(&mut self.overlay_requests)
    }

    /// 对名为 name 的控件的 prop 属性启动补间动画（过渡到 to）。
    pub fn animate(
        &mut self,
        name: impl Into<String>,
        prop: crate::anim::AnimProp,
        to: f32,
        dur_secs: f32,
        easing: crate::anim::Easing,
    ) {
        self.anim_requests.push(AnimRequest { name: name.into(), prop, to, dur_secs, easing });
    }

    /// 后端取走本轮动画请求（随后交给分发器 animate）。
    pub fn take_anim_requests(&mut self) -> Vec<AnimRequest> {
        std::mem::take(&mut self.anim_requests)
    }

    // —— 控件访问（复用 EventCtx 能力）——
    pub fn set_text(&mut self, name: &str, text: impl Into<String>) {
        EventCtx::new(self.root).set_text(name, text);
    }
    pub fn is_selected(&mut self, name: &str) -> Option<bool> {
        EventCtx::new(self.root).is_selected(name)
    }
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        EventCtx::new(self.root).set_enabled(name, enabled);
    }
    pub fn with<R>(&mut self, name: &str, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        EventCtx::new(self.root).with(name, f)
    }

    // —— 窗口控制 ——
    pub fn set_title(&mut self, title: &str) {
        self.win.set_title(title);
    }
    pub fn close(&mut self) {
        self.win.close();
    }
    pub fn minimize(&mut self) {
        self.win.minimize();
    }
    pub fn maximize(&mut self) {
        self.win.maximize();
    }
    pub fn restore(&mut self) {
        self.win.restore();
    }
}

/// 窗口运行期委托（由 facade 的 WindowImpl 适配后传给后端）。
/// 所有钩子有默认空实现，≈ WindowImplBase 的虚方法。
pub trait WindowDelegate {
    /// 窗口与控件创建完成（≈ InitWindow）。
    fn on_init(&mut self, _ctx: &mut WindowCtx) {}
    /// 某个具名控件被点击（≈ Notify 的 click）。
    fn on_activate(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 某个具名控件被双击。
    fn on_double_click(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 某个具名控件被右键（供上下文菜单），坐标为逻辑像素。
    fn on_context(&mut self, _name: &str, _x: f32, _y: f32, _ctx: &mut WindowCtx) {}
    /// 窗口尺寸变化。
    fn on_size(&mut self, _width: f32, _height: f32, _ctx: &mut WindowCtx) {}
    /// 键盘按下（平台无关键码）。
    fn on_key(&mut self, _key: u32, _ctx: &mut WindowCtx) {}
    /// 后台线程经 `MainProxy` 投递的消息（主线程处理）。
    fn on_message(&mut self, _msg: &str, _ctx: &mut WindowCtx) {}
    /// 文件拖放到窗口（paths 为被拖入的文件/目录绝对路径）。
    fn on_drop_files(&mut self, _paths: &[String], _ctx: &mut WindowCtx) {}
    /// 关闭请求；返回 false 可阻止关闭。
    fn on_close(&mut self, _ctx: &mut WindowCtx) -> bool {
        true
    }
}

/// 空委托：所有钩子用默认实现。供不需要窗口钩子的底层调用方（FFI/示例）使用。
pub struct NoopDelegate;
impl WindowDelegate for NoopDelegate {}
