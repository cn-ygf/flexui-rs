//! 窗口 OO 抽象（L3）。平台无关的窗口配置、控制句柄、上下文与委托接口。
//!
//! 对齐 duilib：`WindowDelegate` ≈ WindowImplBase 的运行期钩子；`WindowHandle` ≈ Window
//! 的控制 API；`WindowCtx` ≈ 传给钩子、可 FindControl + 控制窗口的上下文。
//! 用户面向的 `WindowImpl`/`Window`/`Skin` 在 facade（flexui）里，本模块是其底座。

use crate::dispatch::{EventCtx, Invalidation};
use crate::frame_animation::{FrameAnimation, FrameLayer};
use crate::widget::Widget;
use crate::widget::{Node, WidgetProperty, WidgetPropertyKey};
use crate::{ControlEvent, WindowEvent};
use flexui_geometry::Rect;
use flexui_i18n::{LocalizedStringResource, Localizer};

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

/// 无边框窗口的内容区拖动策略。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WindowDragRegion {
    /// 沿用平台原有的无边框窗口拖动行为。
    #[default]
    PlatformDefault,
    /// 禁止从内容区拖动窗口。
    Disabled,
    /// 仅允许从指定逻辑坐标矩形内的空白区域拖动窗口。
    Rect(Rect),
}

/// 窗口配置。
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    /// 窗口标题的本地化资源；切换语言时同步更新原生标题。
    pub localized_title: Option<LocalizedStringResource>,
    pub width: f32,
    pub height: f32,
    /// false = 禁止改变大小。
    pub resizable: bool,
    pub titlebar: TitlebarMode,
    /// 无边框窗口是否使用平台提供的窗口圆角；平台无法关闭时遵循系统行为。
    pub system_corners: bool,
    /// 是否使用平台提供的窗口阴影。
    pub system_shadow: bool,
    /// 无边框窗口内容区的可拖动范围。
    pub drag_region: WindowDragRegion,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "flexui-rs".to_string(),
            localized_title: None,
            width: 640.0,
            height: 440.0,
            resizable: true,
            titlebar: TitlebarMode::System,
            system_corners: true,
            system_shadow: true,
            drag_region: WindowDragRegion::PlatformDefault,
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
    pub fn system_corners(mut self, v: bool) -> Self {
        self.system_corners = v;
        self
    }
    pub fn system_shadow(mut self, v: bool) -> Self {
        self.system_shadow = v;
        self
    }
    pub fn drag_region(mut self, region: WindowDragRegion) -> Self {
        self.drag_region = region;
        self
    }
    pub fn drag_area(mut self, rect: Rect) -> Self {
        self.drag_region = WindowDragRegion::Rect(rect);
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
    /// 在系统原生菜单中同步等待选择；取消时返回 None。
    fn popup_native_menu(
        &mut self,
        _menu: &crate::NativeMenu,
        _anchor: crate::NativeMenuPopupAnchor,
    ) -> Option<String> {
        None
    }
    /// 共享环境发生变化；平台可立即通知同一应用中的其他窗口。
    fn environment_changed(&mut self) {}
}

/// 上下文菜单等浮层的打开请求（由 WindowCtx 收集，后端排空后交分发器）。
pub struct OverlayRequest {
    /// 锚点矩形（右键点位可用 0 尺寸 rect）。
    pub anchor: flexui_geometry::Rect,
    /// 菜单项 (标签, name)；选中后按 name 经 on_activate 上报。
    pub items: Vec<(String, String)>,
    /// 自定义菜单外观；None 使用框架默认值。
    pub style: Option<crate::widgets::MenuStyle>,
    /// 当前选中项的 name；菜单项左侧绘制勾选标记。
    pub selected_name: Option<String>,
    /// 带图标或子菜单时使用；None 表示使用兼容的 items。
    pub entries: Option<Vec<crate::widgets::MenuEntry>>,
}

/// 属性动画请求（由 WindowCtx 收集，后端排空后交分发器）。
pub struct AnimRequest {
    pub name: String,
    pub prop: crate::anim::AnimProp,
    pub to: f32,
    pub dur_secs: f32,
    pub easing: crate::anim::Easing,
}

/// 一个待创建窗口的完整规格（多窗口用）。后端据此建原生窗口并接入共享事件循环。
pub struct NewWindow {
    pub config: WindowConfig,
    pub root: crate::widget::Node,
    pub disp: crate::dispatch::Dispatcher,
    pub delegate: Box<dyn WindowDelegate>,
    pub presentation: WindowPresentation,
    /// 当前窗口使用的共享本地化环境。
    pub localizer: Option<Localizer>,
    /// 最近一次应用到该窗口的目录修订号。
    pub locale_revision: u64,
}

/// 动态窗口的呈现方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowPresentation {
    /// 独立的普通顶层窗口。
    #[default]
    Normal,
    /// 以当前窗口为 owner 的模态工具窗口，不创建新的任务栏入口。
    ModalDialog,
}

/// 传给窗口钩子的上下文：既能按 name 访问控件树，也能控制窗口、弹出菜单。
/// ≈ duilib 里 InitWindow/Notify 内 `FindControl(...)` + `pWnd->Close()` 的能力。
pub struct WindowCtx<'a> {
    root: &'a mut dyn Widget,
    win: &'a mut dyn WindowHandle,
    overlay_requests: Vec<OverlayRequest>,
    anim_requests: Vec<AnimRequest>,
    new_windows: Vec<NewWindow>,
    proxy: Option<crate::dispatch::MainProxy>,
    localizer: Option<Localizer>,
    invalidation: Invalidation,
}

impl<'a> WindowCtx<'a> {
    pub fn new(root: &'a mut dyn Widget, win: &'a mut dyn WindowHandle) -> Self {
        Self {
            root,
            win,
            overlay_requests: Vec::new(),
            anim_requests: Vec::new(),
            new_windows: Vec::new(),
            proxy: None,
            localizer: None,
            invalidation: Invalidation::None,
        }
    }

    /// 带共享本地化环境构造；平台后端用于所有窗口回调。
    pub fn with_localizer(
        root: &'a mut dyn Widget,
        win: &'a mut dyn WindowHandle,
        localizer: Option<Localizer>,
    ) -> Self {
        let mut ctx = Self::new(root, win);
        ctx.localizer = localizer;
        ctx
    }

    /// 在回调里打开一个新窗口（接入共享事件循环）。用 `flexui::build_window(imp)` 构造规格。
    pub fn open_window(&mut self, spec: NewWindow) {
        let mut spec = spec;
        if spec.localizer.is_none() {
            spec.localizer = self.localizer.clone();
        }
        if let Some(theme) = self.theme() {
            crate::apply_theme(spec.root.as_mut(), &theme);
        }
        self.new_windows.push(spec);
    }

    /// 打开以当前窗口为 owner 的模态对话框。
    pub fn open_modal(&mut self, mut spec: NewWindow) {
        spec.presentation = WindowPresentation::ModalDialog;
        if spec.localizer.is_none() {
            spec.localizer = self.localizer.clone();
        }
        if let Some(theme) = self.theme() {
            crate::apply_theme(spec.root.as_mut(), &theme);
        }
        self.new_windows.push(spec);
    }

    /// 后端取走本轮待创建窗口。
    pub fn take_new_windows(&mut self) -> Vec<NewWindow> {
        std::mem::take(&mut self.new_windows)
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

    pub fn with_proxy_and_localizer(
        root: &'a mut dyn Widget,
        win: &'a mut dyn WindowHandle,
        proxy: crate::dispatch::MainProxy,
        localizer: Option<Localizer>,
    ) -> Self {
        let mut ctx = Self::with_localizer(root, win, localizer);
        ctx.proxy = Some(proxy);
        ctx
    }

    /// 运行时切换 BCP 47 语言；所有共享该环境的窗口会同步刷新。
    pub fn set_locale(&mut self, locale: &str) -> Result<(), flexui_i18n::I18nError> {
        self.localizer
            .as_ref()
            .ok_or_else(|| flexui_i18n::I18nError::Format("窗口未配置本地化环境".into()))?
            .set_locale(locale)?;
        self.win.environment_changed();
        Ok(())
    }

    /// 恢复跟随操作系统语言。
    pub fn set_system_locale(&mut self) -> Result<(), flexui_i18n::I18nError> {
        let localizer = self
            .localizer
            .as_ref()
            .ok_or_else(|| flexui_i18n::I18nError::Format("窗口未配置本地化环境".into()))?;
        localizer.set_system_locale();
        self.win.environment_changed();
        Ok(())
    }

    pub fn locale(&self) -> Option<String> {
        self.localizer.as_ref().map(Localizer::locale)
    }

    /// 仅切换当前窗口主题，并自动重新布局和重绘。
    pub fn set_theme(&mut self, theme: crate::Theme) {
        crate::apply_theme(self.root, &theme);
        self.invalidation = self.invalidation.merge(Invalidation::Layout);
    }

    pub fn theme(&self) -> Option<crate::Theme> {
        self.root
            .base()
            .applied_theme
            .as_ref()
            .map(|theme| theme.as_ref().clone())
    }

    /// 使用当前窗口的环境解析本地化资源，适合动态状态文案。
    pub fn localized_text(&self, resource: impl Into<LocalizedStringResource>) -> String {
        let resource = resource.into();
        self.localizer
            .as_ref()
            .map(|localizer| localizer.text(resource.clone()))
            .unwrap_or_else(|| {
                resource
                    .default_value
                    .unwrap_or_else(|| resource.key.as_str().to_owned())
            })
    }

    /// 设置控件文本并保留本地化绑定，后续切换语言时会继续刷新。
    pub fn set_localized_text(&mut self, name: &str, resource: impl Into<LocalizedStringResource>) {
        let resource = resource.into();
        let text = self.localized_text(resource.clone());
        self.with(name, move |widget| {
            widget
                .base_mut()
                .localizations
                .retain(|binding| !matches!(binding, crate::LocalizationBinding::Text(_)));
            widget
                .base_mut()
                .localizations
                .push(crate::LocalizationBinding::Text(resource));
            widget.set_text_value(text);
        });
    }

    /// 取 UI 线程投递句柄；可 clone 给线程或 async 任务后调用 `post` 修改属性。
    pub fn main_proxy(&self) -> Option<crate::dispatch::MainProxy> {
        self.proxy.clone()
    }

    /// 在 anchor 处弹出上下文菜单；items 为 (标签, name)。选中项经 on_activate 上报。
    pub fn open_menu(&mut self, anchor: flexui_geometry::Rect, items: Vec<(String, String)>) {
        self.overlay_requests.push(OverlayRequest {
            anchor,
            items,
            style: self
                .theme()
                .map(|theme| crate::widgets::MenuStyle::from_theme(&theme)),
            selected_name: None,
            entries: None,
        });
    }

    /// 在 anchor 处弹出带自定义外观和当前选中项的菜单。
    pub fn open_styled_menu(
        &mut self,
        anchor: flexui_geometry::Rect,
        items: Vec<(String, String)>,
        style: crate::widgets::MenuStyle,
        selected_name: Option<String>,
    ) {
        self.overlay_requests.push(OverlayRequest {
            anchor,
            items,
            style: Some(style),
            selected_name,
            entries: None,
        });
    }

    /// 在 anchor 处弹出带图标、子菜单和自定义外观的菜单。
    pub fn open_styled_menu_entries(
        &mut self,
        anchor: flexui_geometry::Rect,
        entries: Vec<crate::widgets::MenuEntry>,
        style: crate::widgets::MenuStyle,
    ) {
        self.overlay_requests.push(OverlayRequest {
            anchor,
            items: Vec::new(),
            style: Some(style),
            selected_name: None,
            entries: Some(entries),
        });
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
        self.anim_requests.push(AnimRequest {
            name: name.into(),
            prop,
            to,
            dur_secs,
            easing,
        });
    }

    /// 后端取走本轮动画请求（随后交给分发器 animate）。
    pub fn take_anim_requests(&mut self) -> Vec<AnimRequest> {
        std::mem::take(&mut self.anim_requests)
    }

    /// 后端取走本轮属性修改产生的刷新请求。
    pub fn take_invalidation(&mut self) -> Invalidation {
        std::mem::take(&mut self.invalidation)
    }

    fn event_ctx<R>(&mut self, f: impl FnOnce(&mut EventCtx) -> R) -> R {
        let mut ctx = EventCtx::new(self.root);
        let result = f(&mut ctx);
        self.invalidation = self.invalidation.merge(ctx.take_invalidation());
        result
    }

    // —— 控件访问（复用 EventCtx 能力）——
    pub fn set_text(&mut self, name: &str, text: impl Into<String>) {
        self.event_ctx(|ctx| ctx.set_text(name, text));
    }
    pub fn text(&mut self, name: &str) -> Option<String> {
        self.event_ctx(|ctx| ctx.text(name))
    }
    pub fn is_selected(&mut self, name: &str) -> Option<bool> {
        self.event_ctx(|ctx| ctx.is_selected(name))
    }
    pub fn set_selected(&mut self, name: &str, selected: bool) -> bool {
        self.event_ctx(|ctx| ctx.set_selected(name, selected))
    }
    pub fn selected_index(&mut self, name: &str) -> Option<usize> {
        self.event_ctx(|ctx| ctx.selected_index(name))
    }
    pub fn set_selected_index(&mut self, name: &str, index: usize) -> bool {
        self.event_ctx(|ctx| ctx.set_selected_index(name, index))
    }
    pub fn value(&mut self, name: &str) -> Option<f32> {
        self.event_ctx(|ctx| ctx.value(name))
    }
    pub fn set_value(&mut self, name: &str, value: f32) -> bool {
        self.event_ctx(|ctx| ctx.set_value(name, value))
    }
    pub fn scroll_offset(&mut self, name: &str) -> Option<flexui_geometry::Point> {
        self.event_ctx(|ctx| ctx.scroll_offset(name))
    }
    /// 让 VirtualList 等惰性数据控件重新读取数据源。
    pub fn refresh_data(&mut self, name: &str) -> bool {
        self.event_ctx(|ctx| ctx.refresh_data(name))
    }
    pub fn is_enabled(&mut self, name: &str) -> Option<bool> {
        self.event_ctx(|ctx| ctx.is_enabled(name))
    }
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        self.event_ctx(|ctx| ctx.set_enabled(name, enabled));
    }
    pub fn is_visible(&mut self, name: &str) -> Option<bool> {
        self.event_ctx(|ctx| ctx.is_visible(name))
    }
    pub fn set_visible(&mut self, name: &str, visible: bool) {
        self.event_ctx(|ctx| ctx.set_visible(name, visible));
    }
    pub fn set_property(&mut self, name: &str, property: WidgetProperty) -> bool {
        self.event_ctx(|ctx| ctx.set_property(name, property))
    }
    pub fn property(&mut self, name: &str, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        self.event_ctx(|ctx| ctx.property(name, key))
    }
    pub fn add_child(&mut self, name: &str, child: Node) -> bool {
        self.event_ctx(|ctx| ctx.add_child(name, child))
    }
    pub fn remove_child(&mut self, name: &str, index: usize) -> Option<Node> {
        self.event_ctx(|ctx| ctx.remove_child(name, index))
    }
    pub fn replace_child(&mut self, name: &str, index: usize, child: Node) -> Option<Node> {
        self.event_ctx(|ctx| ctx.replace_child(name, index, child))
    }
    pub fn clear_children(&mut self, name: &str) -> Option<Vec<Node>> {
        self.event_ctx(|ctx| ctx.clear_children(name))
    }
    pub fn with<R>(&mut self, name: &str, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        self.event_ctx(|ctx| ctx.with(name, f))
    }
    pub fn get<R>(&mut self, name: &str, f: impl FnOnce(&dyn Widget) -> R) -> Option<R> {
        self.event_ctx(|ctx| ctx.get(name, f))
    }
    pub fn with_paint<R>(&mut self, name: &str, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        self.event_ctx(|ctx| ctx.with_paint(name, f))
    }
    pub fn request_paint(&mut self, name: &str) -> bool {
        self.event_ctx(|ctx| ctx.request_paint(name))
    }
    pub fn request_redraw(&mut self) {
        self.event_ctx(|ctx| ctx.request_redraw());
    }
    pub fn request_layout(&mut self) {
        self.event_ctx(|ctx| ctx.request_layout());
    }
    pub fn play_frame_animation(
        &mut self,
        name: &str,
        layer: FrameLayer,
        animation: FrameAnimation,
    ) -> bool {
        self.event_ctx(|ctx| ctx.play_frame_animation(name, layer, animation))
    }
    pub fn pause_frame_animation(&mut self, name: &str, layer: FrameLayer) -> bool {
        self.event_ctx(|ctx| ctx.pause_frame_animation(name, layer))
    }
    pub fn resume_frame_animation(&mut self, name: &str, layer: FrameLayer) -> bool {
        self.event_ctx(|ctx| ctx.resume_frame_animation(name, layer))
    }
    pub fn stop_frame_animation(&mut self, name: &str, layer: FrameLayer) -> bool {
        self.event_ctx(|ctx| ctx.stop_frame_animation(name, layer))
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
    /// 弹出 AppKit/Win32 原生菜单，并同步返回被选择的命令 ID。
    pub fn popup_native_menu(
        &mut self,
        menu: &crate::NativeMenu,
        anchor: crate::NativeMenuAnchor,
    ) -> Option<String> {
        let anchor = match anchor {
            crate::NativeMenuAnchor::Cursor => crate::NativeMenuPopupAnchor::Cursor,
            crate::NativeMenuAnchor::Window(point) => crate::NativeMenuPopupAnchor::Window(point),
            crate::NativeMenuAnchor::Screen(point) => crate::NativeMenuPopupAnchor::Screen(point),
            crate::NativeMenuAnchor::Control(name) => {
                let rect = self.get(&name, |widget| widget.base().rect)?;
                crate::NativeMenuPopupAnchor::Window(flexui_geometry::Point::new(
                    rect.left(),
                    rect.bottom(),
                ))
            }
        };
        self.win.popup_native_menu(menu, anchor)
    }
}

/// 窗口运行期委托（由 facade 的 WindowImpl 适配后传给后端）。
/// 所有钩子有默认空实现，≈ WindowImplBase 的虚方法。
pub trait WindowDelegate {
    /// 平台窗口和控件树已建立、应用初始化逻辑尚未执行。
    fn on_before_init(&mut self, _ctx: &mut WindowCtx) {}
    /// 窗口初始化（≈ InitWindow）；适合设置初始属性和注册后台任务。
    fn on_init(&mut self, _ctx: &mut WindowCtx) {}
    /// `on_init` 已执行完成，窗口即将显示首帧。
    fn on_initialized(&mut self, _ctx: &mut WindowCtx) {}
    /// 某个具名控件被点击（≈ Notify 的 click）。
    fn on_activate(&mut self, _name: &str, _ctx: &mut WindowCtx) {}
    /// 具名控件的 hover、focus、文本、选择和值等语义变化。
    fn on_control_event(&mut self, _name: &str, _event: &ControlEvent, _ctx: &mut WindowCtx) {}
    /// 窗口焦点、尺寸、DPI、键盘及窗口状态等统一事件。
    fn on_window_event(&mut self, _event: &WindowEvent, _ctx: &mut WindowCtx) {}
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
    /// 即将关闭；返回 false 可阻止关闭。默认转发旧版 `on_close` 钩子。
    fn on_closing(&mut self, ctx: &mut WindowCtx) -> bool {
        self.on_close(ctx)
    }
    /// 兼容旧版关闭钩子；新代码优先实现 `on_closing`。
    fn on_close(&mut self, _ctx: &mut WindowCtx) -> bool {
        true
    }
    /// 原生窗口已经关闭；此时窗口与控件上下文不再可用，只适合释放业务资源。
    fn on_closed(&mut self) {}
}

/// 空委托：所有钩子用默认实现。供不需要窗口钩子的底层调用方（FFI/示例）使用。
pub struct NoopDelegate;
impl WindowDelegate for NoopDelegate {}
