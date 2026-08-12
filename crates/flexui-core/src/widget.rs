//! 控件基类与 Widget trait（L3/L4）。
//!
//! 采用「组合式基类」：所有控件都内嵌一个 `Base`（承载状态、样式、布局、子控件、
//! 交互数据），具体控件只覆写 `measure/arrange/paint_content/on_event`。
//! 这样统一绘制/布局/分发管线可对任意控件生效，减少每控件样板。

use std::sync::atomic::{AtomicU64, Ordering};

use flexui_geometry::{Insets, Rect, Size};
use flexui_gfx::{Canvas, Font};

use crate::anim::AnimProp;
use crate::event::{Event, EventFlow};
use crate::sizing::{Align, Justify, Sizing};
use crate::style::{BaseState, PlaceholderStyleSet, StyleSet, StyleSpec, VisualState};

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
    /// 滑块（拖动改变 value）。
    Slider,
    /// 下拉选择框（点击弹出选项菜单）。
    ComboBox,
    /// 菜单项（浮层菜单中的一行）。
    MenuItem,
    /// 列表视图（可滚动，点击选中行）。
    ListView,
}

/// XML/构建器写入控件专属配置的类型安全入口。
pub enum WidgetProperty {
    Placeholder(String),
    PlaceholderStyle(PlaceholderStyleSet),
    Multiline(bool),
    ReadOnly(bool),
    NumberOnly(bool),
    Password(bool),
    PasswordChar(char),
    MaxChars(Option<usize>),
    AutoSelectAll(bool),
    SwitchStyle(bool),
    Group(Option<u32>),
    TabIndex(Option<usize>),
    SelectedIndex(usize),
    Value(f32),
}

/// 平台文本输入协议需要的只读快照。
pub struct TextInputState {
    pub text: String,
    pub cursor: usize,
    pub selection: Option<(usize, usize)>,
    pub marked: String,
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
    /// 悬停提示文本（Tooltip）；None 表示无提示。
    pub tooltip: Option<String>,
    pub font: Font,
    pub style: StyleSet,

    // —— 运行时交互状态（由分发器维护）——
    pub enabled: bool,
    pub hover: bool,
    pub pressed: bool,
    pub focused: bool,
    pub focusable: bool,
    /// 任一后代获得焦点时，本节点及其子树使用 focus 状态样式。
    pub focus_within: bool,
    /// 光标闪烁相位（true=显示），由分发器的 blink 定时切换；Edit 绘制光标用。
    pub caret_on: bool,
    pub visible: bool,
    pub hit: HitPolicy,

    // —— 选择 / 分组（Radio/CheckBox/TabBox 用）——
    pub selected: bool,

    // —— 布局 ——
    /// 布局后的绝对矩形（窗口逻辑坐标）。
    pub rect: Rect,
    pub padding: Insets,
    /// 外边距（参与容器排布）。
    pub margin: Insets,
    /// 宽/高尺寸模式（Fixed/Content/Fill）。
    pub width: Sizing,
    pub height: Sizing,
    /// Fill 时的分配权重（0 视为 1）。
    pub flex_grow: f32,
    /// Box/VBox/HBox 子控件间距。
    pub spacing: f32,
    /// 容器主轴对齐（VBox/HBox）。
    pub justify: Justify,
    /// 容器交叉轴对齐（VBox/HBox）。
    pub align: Align,
    /// 绝对定位（相对父内容区左上角）。设了则 Box 按此摆放，忽略叠放填充。
    pub pos: Option<(f32, f32)>,

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
            tooltip: None,
            font: Font::default(),
            style: StyleSet::new(),
            enabled: true,
            hover: false,
            pressed: false,
            focused: false,
            focus_within: false,
            caret_on: true,
            focusable: matches!(
                role,
                WidgetRole::Button
                    | WidgetRole::Radio
                    | WidgetRole::CheckBox
                    | WidgetRole::Edit
                    | WidgetRole::ComboBox
            ),
            visible: true,
            hit: HitPolicy::Solid,
            selected: false,
            rect: Rect::default(),
            padding: Insets::default(),
            margin: Insets::default(),
            width: Sizing::Content,
            height: Sizing::Content,
            flex_grow: 0.0,
            spacing: 0.0,
            justify: Justify::Start,
            align: Align::Stretch,
            pos: None,
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

    /// 当前完整视觉状态（含 selected 维度，供勾选/单选贴图）。
    pub fn visual_state(&self) -> VisualState {
        VisualState::with_selected(self.effective_base(), self.focused, self.selected)
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

    /// 子控件绘制与命中的可见区域；滚动容器覆写为去除 padding 后的视口。
    fn children_viewport(&self) -> Rect { self.base().rect }

    /// 子控件绘制完成后的前景层；滚动条等需要覆盖在内容之上的元素使用。
    fn paint_foreground(&self, _cv: &mut dyn Canvas, _style: &StyleSpec) {}

    /// 自定义事件处理（默认不拦截）。
    fn on_event(&mut self, _ev: &Event) -> EventFlow {
        EventFlow::Ignored
    }

    /// 应用控件专属配置；不支持该属性时返回 false。
    fn apply_property(&mut self, _property: WidgetProperty) -> bool { false }

    /// 获得焦点后的控件专属行为。
    fn focus_gained(&mut self) {}

    // —— 文本编辑钩子（供分发器统一驱动复制/剪切/粘贴/全选，Edit 覆写）——
    /// 当前选中的文本（无选区返回 None）。供复制/剪切读取。
    fn selected_text(&self) -> Option<String> {
        None
    }
    /// 用 s 替换当前选区（无选区则在光标处插入）。返回内容是否改变。供粘贴用。
    fn replace_selection(&mut self, _s: &str) -> bool {
        false
    }
    /// 删除当前选区。返回是否存在选区被删除。供剪切用。
    fn delete_selection(&mut self) -> bool {
        false
    }
    /// 全选文本。
    fn select_all(&mut self) {}

    /// 统一设置文本，Edit 可覆写以同步内部编辑状态。
    fn set_text_value(&mut self, text: String) { self.base_mut().text = text; }

    /// 排版后的文本插入点，供平台输入法定位候选窗口。
    fn text_input_rect(&self) -> Option<Rect> { None }

    /// 平台文本输入协议快照与组合串写入。
    fn text_input_state(&self) -> Option<TextInputState> { None }
    fn set_marked_text(&mut self, _text: String) -> bool { false }
    fn clear_marked_text(&mut self) -> bool { false }

    /// 控件专属选择/分组能力。
    fn selection_group(&self) -> Option<u32> { None }
    fn tab_index(&self) -> Option<usize> { None }
    fn selected_index(&self) -> Option<usize> { None }
    fn set_selected_index(&mut self, _index: usize) -> bool { false }

    /// 滚动与动画能力。
    fn is_scrollable(&self) -> bool { false }
    fn scroll_by(&mut self, _dy: f32) -> bool { false }
    fn scroll_position(&self) -> Option<f32> { None }
    fn animation_value(&self, _prop: AnimProp) -> Option<f32> { None }
    fn set_animation_value(&mut self, _prop: AnimProp, _value: f32) -> bool { false }

    // —— 下拉/菜单钩子（供分发器打开选项菜单，ComboBox 覆写）——
    /// 该控件点击时要弹出的菜单项文本；返回 None 表示不弹菜单。
    fn menu_items(&self) -> Option<Vec<String>> {
        None
    }
    /// 选中第 i 项（ComboBox：设 selected_index 并回填文本）。
    fn set_selected_item(&mut self, _i: usize) {}
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
        self.set_text_value(text.into());
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

/// 按 id 查找控件并返回可变引用（首个匹配）。供后端 IME 等按焦点 id 定位控件用。
pub fn find_mut_by_id(node: &mut dyn Widget, id: WidgetId) -> Option<&mut dyn Widget> {
    if node.base().id == id {
        return Some(node);
    }
    for child in node.base_mut().children.iter_mut() {
        if let Some(found) = find_mut_by_id(child.as_mut(), id) {
            return Some(found);
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
