//! 控件基类与 Widget trait（L3/L4）。
//!
//! 采用「组合式基类」：所有控件都内嵌一个 `Base`（承载状态、样式、布局、子控件、
//! 交互数据），具体控件只覆写 `measure/arrange/paint_content/on_event`。
//! 这样统一绘制/布局/分发管线可对任意控件生效，减少每控件样板。

use std::sync::atomic::{AtomicU64, Ordering};

use flexui_geometry::{Insets, Rect, Size};
use flexui_gfx::{Canvas, Font};

use crate::event::{Event, EventFlow};
use crate::style::{BaseState, StyleSet, StyleSpec, VisualState};

/// 控件唯一 id。
pub type WidgetId = u64;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 分配一个全局唯一 id。
pub fn next_id() -> WidgetId {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// 命中测试策略（对应需求 C6：消息穿透）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitPolicy {
    /// 不穿透：命中即消费。
    Solid,
    /// 穿透：自身不接收，事件穿过去给下层。
    Transparent,
}

/// 控件角色：供事件分发器做通用的选择/分组处理（避免向下转型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetRole {
    /// 普通控件/容器。
    Plain,
    /// 按钮（可点击）。
    Button,
    /// 勾选框（点击切换 selected）。
    CheckBox,
    /// 单选（同 group 互斥；可关联 tab_index 驱动 TabBox）。
    Radio,
    /// 多页容器（按 selected_index 显示某页）。
    TabBox,
    /// 文本输入。
    Edit,
}

/// 节点类型：装箱的 trait 对象。
pub type Node = Box<dyn Widget>;

/// 控件共享数据。字段偏多是有意为之（组合式基类），
/// 让统一管线（绘制/布局/分发）无需向下转型即可工作。
pub struct Base {
    pub id: WidgetId,
    pub name: Option<String>,
    pub role: WidgetRole,
    /// 文本内容（Label/Button/Edit/CheckBox/Radio 复用）。
    pub text: String,
    pub font: Font,
    pub style: StyleSet,

    // —— 运行时交互状态（由分发器维护）——
    pub enabled: bool,
    pub hover: bool,
    pub pressed: bool,
    pub focused: bool,
    pub focusable: bool,
    pub visible: bool,
    pub hit: HitPolicy,

    // —— 选择 / 分组（Radio/CheckBox/TabBox 用）——
    pub selected: bool,
    pub group: Option<u32>,
    pub tab_index: Option<usize>,
    pub selected_index: usize,

    // —— 布局 ——
    /// 布局后的绝对矩形（窗口逻辑坐标）。
    pub rect: Rect,
    pub padding: Insets,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub flex_grow: f32,
    /// Box/VBox/HBox 子控件间距。
    pub spacing: f32,

    // —— 子控件 ——
    pub children: Vec<Node>,

    // —— 回调（点击时触发，参数为事件上下文，可按 name 访问整棵控件树）——
    pub on_click: Option<crate::dispatch::ClickHandler>,
}

impl Base {
    pub fn new(role: WidgetRole) -> Self {
        Self {
            id: next_id(),
            name: None,
            role,
            text: String::new(),
            font: Font::default(),
            style: StyleSet::new(),
            enabled: true,
            hover: false,
            pressed: false,
            focused: false,
            focusable: matches!(role, WidgetRole::Button | WidgetRole::Radio | WidgetRole::CheckBox | WidgetRole::Edit),
            visible: true,
            hit: HitPolicy::Solid,
            selected: false,
            group: None,
            tab_index: None,
            selected_index: 0,
            rect: Rect::default(),
            padding: Insets::default(),
            width: None,
            height: None,
            flex_grow: 0.0,
            spacing: 0.0,
            children: Vec::new(),
            on_click: None,
        }
    }

    /// 计算当前生效的基础状态：禁用最高优先，其次按下、悬停、普通。
    pub fn effective_base(&self) -> BaseState {
        if !self.enabled {
            BaseState::Disabled
        } else if self.pressed {
            BaseState::Pushed
        } else if self.hover {
            BaseState::Hot
        } else {
            BaseState::Normal
        }
    }

    /// 当前完整视觉状态。
    pub fn visual_state(&self) -> VisualState {
        VisualState::new(self.effective_base(), self.focused)
    }

    /// 解析当前生效样式。
    pub fn resolved_style(&self) -> StyleSpec {
        self.style.resolve(self.visual_state())
    }
}

/// 所有控件实现的接口。默认实现覆盖「单子/叠放」布局与空内容，
/// 具体控件按需覆写。
pub trait Widget {
    fn base(&self) -> &Base;
    fn base_mut(&mut self) -> &mut Base;

    /// 度量期望尺寸（默认取子控件最大尺寸 + padding，或显式尺寸）。
    fn measure(&mut self, avail: Size, cv: &dyn Canvas) -> Size {
        crate::layout::measure_stack(self.base_mut(), avail, cv)
    }

    /// 摆放子控件（默认让每个子控件填充内容区 = 单子嵌套 / Box 叠放）。
    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        crate::layout::arrange_stack(self.base_mut(), content, cv)
    }

    /// 绘制自身内容（文字/图标等）。背景/边框/子控件由统一管线负责。
    fn paint_content(&self, _cv: &mut dyn Canvas, _style: &StyleSpec) {}

    /// 自定义事件处理（默认不拦截）。
    fn on_event(&mut self, _ev: &Event) -> EventFlow {
        EventFlow::Ignored
    }
}

// —— 能力 trait：以 trait 约束表达「类型层级/继承视图」（组合优于继承的 Rust 表达）——
//
// 对照 duilib 的模板继承链 Label→Button→CheckBox→Radio，这里用能力 trait 表示：
//   Widget（基类，人人都实现）
//     ├─ TextControl  有文本：Label / Button / Edit / CheckBox / Radio
//     ├─ Clickable    可点击：Button / CheckBox / Radio
//     └─ Container     容器：Panel(Box) / VBox / HBox / TabBox

/// 有文本内容的控件。
pub trait TextControl: Widget {
    fn text(&self) -> &str {
        &self.base().text
    }
    fn set_text(&mut self, text: impl Into<String>)
    where
        Self: Sized,
    {
        self.base_mut().text = text.into();
    }
}

/// 可点击控件。
pub trait Clickable: Widget {}

/// 可容纳子控件的容器。
pub trait Container: Widget {
    /// 追加一个已装箱子控件。
    fn add(&mut self, child: Node)
    where
        Self: Sized,
    {
        self.base_mut().children.push(child);
    }
    /// 子控件数量。
    fn child_count(&self) -> usize {
        self.base().children.len()
    }
}

/// 前序遍历整棵控件树并对每个节点执行 f（供上层/FFI 批量配置控件用）。
pub fn visit_all_mut(node: &mut dyn Widget, f: &mut dyn FnMut(&mut dyn Widget)) {
    f(node);
    let n = node.base().children.len();
    for i in 0..n {
        visit_all_mut(node.base_mut().children[i].as_mut(), f);
    }
}

/// 按 name 查找控件 id（返回首个匹配）。
pub fn find_by_name(node: &dyn Widget, name: &str) -> Option<WidgetId> {
    if node.base().name.as_deref() == Some(name) {
        return Some(node.base().id);
    }
    for child in node.base().children.iter() {
        if let Some(id) = find_by_name(child.as_ref(), name) {
            return Some(id);
        }
    }
    None
}

/// 按 name 查找控件并返回可变引用（首个匹配）。供动态构建后直接修改控件用（W8）。
pub fn find_mut_by_name<'a>(node: &'a mut dyn Widget, name: &str) -> Option<&'a mut dyn Widget> {
    if node.base().name.as_deref() == Some(name) {
        return Some(node);
    }
    for child in node.base_mut().children.iter_mut() {
        if let Some(found) = find_mut_by_name(child.as_mut(), name) {
            return Some(found);
        }
    }
    None
}
