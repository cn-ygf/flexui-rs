//! 控件基类与 Widget trait（L3/L4）。
//!
//! 采用「组合式基类」：所有控件都内嵌一个 `Base`（承载状态、样式、布局、子控件、
//! 交互数据），具体控件只覆写 `measure/arrange/paint_content/on_event`。
//! 这样统一绘制/布局/分发管线可对任意控件生效，减少每控件样板。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use flexui_gfx::{Insets, Rect, Size};
use flexui_gfx::{Canvas, Font, ImageSource};

use crate::anim::AnimProp;
use crate::event::{Event, EventFlow};
use crate::frame_animation::{FrameAnimation, FramePlayer};
use crate::localization::LocalizationBinding;
use crate::sizing::{Align, Justify, Sizing};
use crate::style::{BaseState, PlaceholderStyleSet, StyleSet, StyleSpec, VisualState};
use crate::theme::{Theme, ThemeColorBinding, WidgetKind};

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

/// 控件专属属性名，用于运行时读取 XML 对应属性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetPropertyKey {
    Text,
    Tooltip,
    Font,
    Style,
    Variant,
    Classes,
    Width,
    Height,
    Padding,
    Margin,
    Spacing,
    Flex,
    Position,
    Justify,
    Align,
    Enabled,
    Visible,
    Focusable,
    FocusWithin,
    HitPolicy,
    Placeholder,
    PlaceholderStyle,
    Multiline,
    ReadOnly,
    NumberOnly,
    Password,
    PasswordChar,
    MaxChars,
    AutoSelectAll,
    IndicatorVisible,
    Group,
    TabIndex,
    Selected,
    SelectedIndex,
    Value,
    Items,
    ImageSource,
    Icon,
    IconRect,
    TextRect,
    RowHeight,
    Vertical,
    Thickness,
    BindGroup,
    ScrollBar,
    AutoScroll,
    VirtualColumns,
    VirtualSource,
    VirtualSelectionMode,
    VirtualSelectedRows,
    HeaderHeight,
    ShowHeader,
    Striped,
    FillLastColumn,
    Overscan,
}

/// XML/构建器及运行时写入控件专属配置的类型安全入口。
///
/// 各变体大小差异较大，但本枚举总是「构造后立即被 `set_property` 消费」的一次性载体，
/// 不会成批存入集合，故不必为最大变体装箱（装箱只会给每个构造点添麻烦）。
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub enum WidgetProperty {
    Text(String),
    Tooltip(Option<String>),
    Font(Font),
    Style(StyleSet),
    Variant(String),
    Classes(Vec<String>),
    Width(Sizing),
    Height(Sizing),
    Padding(Insets),
    Margin(Insets),
    Spacing(f32),
    Flex(f32),
    Position(Option<(f32, f32)>),
    Justify(Justify),
    Align(Align),
    Enabled(bool),
    Visible(bool),
    Focusable(bool),
    FocusWithin(bool),
    HitPolicy(HitPolicy),
    Placeholder(String),
    PlaceholderStyle(PlaceholderStyleSet),
    Multiline(bool),
    /// Label 自动换行的最大宽度（像素）；None 表示单行不换行。
    WrapWidth(Option<f32>),
    ReadOnly(bool),
    NumberOnly(bool),
    Password(bool),
    PasswordChar(char),
    MaxChars(Option<usize>),
    AutoSelectAll(bool),
    IndicatorVisible(bool),
    Group(Option<u32>),
    TabIndex(Option<usize>),
    Selected(bool),
    SelectedIndex(usize),
    Value(f32),
    Items(Vec<String>),
    ImageSource(ImageSource),
    Icon(Option<ImageSource>),
    IconRect(Option<Rect>),
    TextRect(Option<Rect>),
    RowHeight(f32),
    Vertical(bool),
    Thickness(f32),
    BindGroup(Option<u32>),
    /// 滚动条可见性模式（auto/always/hidden）。
    ScrollBar(crate::scroll::ScrollBarVisibility),
    /// 文本追加后自动滚到底部（日志/聊天场景）。
    AutoScroll(bool),
    /// 虚拟列表列定义。
    VirtualColumns(Vec<crate::widgets::VirtualColumn>),
    /// 虚拟列表惰性数据源。
    VirtualSource(crate::widgets::VirtualListSourceRef),
    VirtualSelectionMode(crate::widgets::VirtualSelectionMode),
    /// 虚拟列表选中行的稳定 ID。
    VirtualSelectedRows(Vec<u64>),
    HeaderHeight(f32),
    ShowHeader(bool),
    Striped(bool),
    FillLastColumn(bool),
    Overscan(usize),
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
    pub kind: WidgetKind,
    /// 主题控件变体，例如 primary、danger、nav。
    pub variant: String,
    /// 可叠加主题类名，按声明顺序覆盖。
    pub classes: Vec<String>,
    /// 文本内容（Label/Button/Edit/CheckBox/Radio 复用）。
    pub text: String,
    /// 悬停提示文本（Tooltip）；None 表示无提示。
    pub tooltip: Option<String>,
    /// 声明式本地化绑定；运行时切换语言时据此重新解析。
    pub localizations: Vec<LocalizationBinding>,
    pub font: Font,
    pub style: StyleSet,
    /// 当前主题计算出的样式层，显式 style 始终覆盖它。
    pub theme_style: StyleSet,
    /// XML `@token` 颜色绑定，切换主题时重新求值。
    pub theme_colors: Vec<ThemeColorBinding>,
    /// 最近应用的主题，供运行时修改 variant/class 后立即重算。
    pub(crate) applied_theme: Option<Arc<Theme>>,
    pub(crate) theme_root: bool,
    /// 点击触发的背景/前景动画定义（覆盖当前状态图片，结束后恢复）。
    pub click_bg_animation: Option<FrameAnimation>,
    pub click_fg_animation: Option<FrameAnimation>,
    pub(crate) bg_frame_player: FramePlayer,
    pub(crate) fg_frame_player: FramePlayer,
    pub(crate) click_bg_frame_player: FramePlayer,
    pub(crate) click_fg_frame_player: FramePlayer,

    // —— 运行时交互状态（由分发器维护）——
    pub enabled: bool,
    pub hover: bool,
    pub pressed: bool,
    pub focused: bool,
    pub focusable: bool,
    /// 文本是否可选中（拖选 + Cmd+C 复制）；开启时控件也会获得焦点。
    pub selectable: bool,
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
    pub on_control_event: Option<crate::dispatch::ControlEventHandler>,
}

impl Base {
    /// 兼容构造：按事件角色推导最接近的控件外观类型。
    pub fn new(role: WidgetRole) -> Self {
        let kind = match role {
            WidgetRole::Button => WidgetKind::Button,
            WidgetRole::CheckBox => WidgetKind::CheckBox,
            WidgetRole::Radio => WidgetKind::Radio,
            WidgetRole::TabBox => WidgetKind::TabBox,
            WidgetRole::Edit => WidgetKind::Edit,
            WidgetRole::Slider => WidgetKind::Slider,
            WidgetRole::ComboBox => WidgetKind::ComboBox,
            WidgetRole::MenuItem => WidgetKind::MenuItem,
            WidgetRole::ListView => WidgetKind::ListView,
            WidgetRole::Plain => WidgetKind::Panel,
        };
        Self::new_kind(role, kind)
    }

    /// 为自定义控件显式指定事件角色和主题外观类型。
    pub fn new_kind(role: WidgetRole, kind: WidgetKind) -> Self {
        Self {
            id: next_id(),
            name: None,
            role,
            kind,
            variant: String::new(),
            classes: Vec::new(),
            text: String::new(),
            tooltip: None,
            localizations: Vec::new(),
            font: Font::default(),
            style: StyleSet::new(),
            theme_style: StyleSet::new(),
            theme_colors: Vec::new(),
            applied_theme: None,
            theme_root: false,
            click_bg_animation: None,
            click_fg_animation: None,
            bg_frame_player: FramePlayer::default(),
            fg_frame_player: FramePlayer::default(),
            click_bg_frame_player: FramePlayer::default(),
            click_fg_frame_player: FramePlayer::default(),
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
            selectable: false,
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
            on_control_event: None,
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
        self.style
            .resolve_over(&self.theme_style, self.visual_state())
    }

    /// 显式层和主题层所有状态可能产生的阴影。
    pub(crate) fn style_shadows(&self) -> impl Iterator<Item = crate::style::Shadow> + '_ {
        self.style.shadows().chain(self.theme_style.shadows())
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
    fn children_viewport(&self) -> Rect {
        self.base().rect
    }

    /// 子控件绘制完成后的前景层；滚动条等需要覆盖在内容之上的元素使用。
    fn paint_foreground(&self, _cv: &mut dyn Canvas, _style: &StyleSpec) {}

    /// 自定义事件处理（默认不拦截）。
    fn on_event(&mut self, _ev: &Event) -> EventFlow {
        EventFlow::Ignored
    }

    /// 应用控件专属配置；不支持该属性时返回 false。
    fn apply_property(&mut self, _property: WidgetProperty) -> bool {
        false
    }

    /// 读取控件专属配置；返回值使用与写入相同的类型。
    fn property(&self, _key: WidgetPropertyKey) -> Option<WidgetProperty> {
        None
    }

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
    fn set_text_value(&mut self, text: String) {
        self.base_mut().text = text;
    }

    /// 排版后的文本插入点，供平台输入法定位候选窗口。
    fn text_input_rect(&self) -> Option<Rect> {
        None
    }

    /// 平台文本输入协议快照与组合串写入。
    fn text_input_state(&self) -> Option<TextInputState> {
        None
    }
    fn set_marked_text(&mut self, _text: String) -> bool {
        false
    }
    fn clear_marked_text(&mut self) -> bool {
        false
    }

    /// 控件专属选择/分组能力。
    fn selection_group(&self) -> Option<u32> {
        None
    }
    fn tab_index(&self) -> Option<usize> {
        None
    }
    fn tab_bind_group(&self) -> Option<u32> {
        None
    }
    fn selected_index(&self) -> Option<usize> {
        None
    }
    fn set_selected_index(&mut self, _index: usize) -> bool {
        false
    }

    /// 多选列表当前选中的稳定行 ID；非多选控件返回 None。
    fn selected_rows(&self) -> Option<Vec<u64>> {
        None
    }

    /// 可排序数据控件的当前排序状态。
    fn sort_state(&self) -> Option<crate::widgets::VirtualSort> {
        None
    }

    /// 数据表当前列定义；用于列宽拖动后的状态持久化。
    fn virtual_columns(&self) -> Option<Vec<crate::widgets::VirtualColumn>> {
        None
    }

    /// 通知惰性数据控件重新读取数据和布局度量。
    fn refresh_data(&mut self) -> bool {
        false
    }

    /// 滚动与动画能力（双轴）。
    fn is_scrollable(&self) -> bool {
        false
    }
    /// 增量滚动（dx 横向、dy 纵向；正值方向与滚轮一致）。返回是否发生变化。
    fn scroll_by(&mut self, _dx: f32, _dy: f32) -> bool {
        false
    }
    /// 当前滚动偏移（横、纵）。
    fn scroll_offset(&self) -> Option<flexui_gfx::Point> {
        None
    }
    /// 开启「粘底」：此后每次布局都把纵向偏移贴到底部，直到用户手动上滚。
    /// 适合聊天等追加式列表——新增内容（含图片异步撑高）后仍保持在最底。
    /// 返回该控件是否支持粘底。
    fn scroll_to_end(&mut self) -> bool {
        false
    }
    /// 命中滚动条滑块则返回抓取信息（供分发器发起拖动）。默认无滚动条。
    fn scrollbar_grab(&self, _pos: flexui_gfx::Point) -> Option<crate::scroll::ScrollGrab> {
        None
    }
    /// 按拖动中的鼠标位置更新滚动偏移。返回是否变化。
    fn scrollbar_drag(&mut self, _pos: flexui_gfx::Point, _grab: &crate::scroll::ScrollGrab) -> bool {
        false
    }
    /// 该点是否落在本控件的滚动条区域（供光标形状判断：滚动条上应显示箭头而非文本光标）。
    fn scrollbar_contains(&self, _pos: flexui_gfx::Point) -> bool {
        false
    }
    fn animation_value(&self, _prop: AnimProp) -> Option<f32> {
        None
    }
    fn set_animation_value(&mut self, _prop: AnimProp, _value: f32) -> bool {
        false
    }

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

/// 按 id 查找控件并返回只读引用。
pub fn find_by_id(node: &dyn Widget, id: WidgetId) -> Option<&dyn Widget> {
    if node.base().id == id {
        return Some(node);
    }
    for child in node.base().children.iter() {
        if let Some(found) = find_by_id(child.as_ref(), id) {
            return Some(found);
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
