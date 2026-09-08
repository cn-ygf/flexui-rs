//! TreeView / TreeNode：可滚动的层级树控件。
//!
//! 树节点是纯数据，不进入全局 Widget 子树；TreeView 把当前展开的节点投影成可见行，
//! 因而层级很深时也只绘制视口内的行。节点使用稳定 ID，展开、收起和滚动不会让选择失效。

use std::sync::atomic::{AtomicU64, Ordering};

use flexui_gfx::{Canvas, Color, Point, Rect, Size, TextAlign};

use crate::anim::AnimProp;
use crate::common_builders;
use crate::event::{keys, Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::scroll::{paint_scrollbars, ScrollAxes, ScrollBarStyle, ScrollState};
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole};

/// 树节点的稳定标识。
pub type TreeNodeId = u64;

static NEXT_TREE_NODE_ID: AtomicU64 = AtomicU64::new(1);

/// 选中行高亮色。
const DEFAULT_SELECTION_COLOR: Color = Color::rgba(0.20, 0.45, 0.95, 0.45);
/// 展开箭头、复选框与文字之间预留的槽宽。
const INDICATOR_SLOT: f32 = 20.0;

/// TreeView 中的一个层级节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    id: TreeNodeId,
    text: String,
    children: Vec<TreeNode>,
    expanded: bool,
    /// `None` 表示该节点不显示复选框；`Some` 是当前勾选状态。
    checked: Option<bool>,
}

impl TreeNode {
    /// 创建节点并自动分配稳定 ID；分支节点默认收起。
    pub fn new(text: impl Into<String>) -> Self {
        Self::with_id(NEXT_TREE_NODE_ID.fetch_add(1, Ordering::Relaxed), text)
    }

    /// 使用业务侧已有的稳定 ID 创建节点。
    pub fn with_id(id: TreeNodeId, text: impl Into<String>) -> Self {
        // 后续自动分配的 ID 越过业务显式 ID，降低同一棵树中误撞 ID 的概率。
        NEXT_TREE_NODE_ID.fetch_max(id.saturating_add(1), Ordering::Relaxed);
        Self {
            id,
            text: text.into(),
            children: Vec::new(),
            expanded: false,
            checked: None,
        }
    }

    pub fn id(&self) -> TreeNodeId {
        self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    pub fn children(&self) -> &[TreeNode] {
        &self.children
    }

    pub fn children_mut(&mut self) -> &mut Vec<TreeNode> {
        &mut self.children
    }

    /// 以 Builder 方式追加一个子节点。
    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    pub fn add_child(&mut self, child: TreeNode) {
        self.children.push(child);
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// 设置展开状态；叶子节点始终保持收起。
    pub fn set_expanded(&mut self, expanded: bool) -> bool {
        let expanded = expanded && !self.children.is_empty();
        let changed = self.expanded != expanded;
        self.expanded = expanded;
        changed
    }

    pub fn is_checkable(&self) -> bool {
        self.checked.is_some()
    }

    pub fn is_checked(&self) -> Option<bool> {
        self.checked
    }

    /// 为单个节点启用或关闭复选框。
    pub fn checkable(mut self, checkable: bool) -> Self {
        self.checked = checkable.then_some(false);
        self
    }

    /// 运行时启用或关闭该节点的复选框。
    pub fn set_checkable(&mut self, checkable: bool) -> bool {
        let next = match (checkable, self.checked) {
            (true, Some(checked)) => Some(checked),
            (true, None) => Some(false),
            (false, _) => None,
        };
        let changed = self.checked != next;
        self.checked = next;
        changed
    }

    /// 启用复选框并设置初始勾选状态。
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// 设置勾选状态；同时让原本没有复选框的节点变为可勾选。
    pub fn set_checked(&mut self, checked: bool) -> bool {
        let changed = self.checked != Some(checked);
        self.checked = Some(checked);
        changed
    }
}

/// 当前可见行的轻量索引，不持有节点引用，便于事件处理中回写树状态。
#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibleRow {
    id: TreeNodeId,
    depth: usize,
    path: Vec<usize>,
}

/// 支持层级、展开/收起、选择、复选框与纵向滚动的树控件。
pub struct TreeView {
    base: Base,
    nodes: Vec<TreeNode>,
    row_height: f32,
    indent: f32,
    selection: Option<TreeNodeId>,
    /// 为 true 时，即便节点没有单独启用复选框，也给每一行显示复选框。
    show_checkboxes: bool,
    scroll: ScrollState,
    scrollbar: ScrollBarStyle,
}

impl TreeView {
    pub fn new() -> Self {
        // 暂时复用 ListView 的事件角色和主题外观：这样无需修改分发器即可获得
        // 指针事件、选择变化上报、焦点和滚动能力。独立枚举项由共享注册层统一接入。
        let mut base = Base::new_kind(WidgetRole::ListView, WidgetKind::ListView);
        base.focusable = true;
        Self {
            base,
            nodes: Vec::new(),
            row_height: 28.0,
            indent: 20.0,
            selection: None,
            show_checkboxes: false,
            scroll: ScrollState::new(ScrollAxes::vertical()),
            scrollbar: ScrollBarStyle::default(),
        }
    }

    pub fn nodes(mut self, nodes: impl IntoIterator<Item = TreeNode>) -> Self {
        self.nodes = nodes.into_iter().collect();
        self.repair_selection();
        self.refresh_metrics();
        self
    }

    pub fn node(mut self, node: TreeNode) -> Self {
        self.nodes.push(node);
        self.refresh_metrics();
        self
    }

    pub fn root_nodes(&self) -> &[TreeNode] {
        &self.nodes
    }

    pub fn root_nodes_mut(&mut self) -> &mut Vec<TreeNode> {
        &mut self.nodes
    }

    /// 直接通过 `root_nodes_mut` / `find_node_mut` 改变层级或展开状态后刷新可见行度量。
    pub fn refresh(&mut self) {
        self.repair_selection();
        self.refresh_metrics();
    }

    pub fn add_node(&mut self, node: TreeNode) {
        self.nodes.push(node);
        self.refresh_metrics();
    }

    /// 给指定父节点追加子节点，并同步滚动度量。
    pub fn add_child(&mut self, parent_id: TreeNodeId, child: TreeNode) -> bool {
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        parent.add_child(child);
        self.refresh_metrics();
        true
    }

    /// 删除指定节点及其整个子树。
    pub fn remove_node(&mut self, id: TreeNodeId) -> Option<TreeNode> {
        let removed = remove_node(&mut self.nodes, id)?;
        self.repair_selection();
        self.refresh_metrics();
        Some(removed)
    }

    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height.max(1.0);
        self.refresh_metrics();
        self
    }

    pub fn indent(mut self, indent: f32) -> Self {
        self.indent = indent.max(0.0);
        self
    }

    /// 为所有节点显示复选框；节点自身的勾选值为空时按未勾选处理。
    pub fn checkboxes(mut self, visible: bool) -> Self {
        self.show_checkboxes = visible;
        self
    }

    pub fn scrollbar(mut self, visibility: crate::scroll::ScrollBarVisibility) -> Self {
        self.scroll.set_visibility(visibility);
        self
    }

    pub fn selection(&self) -> Option<TreeNodeId> {
        self.selection
    }

    /// 以 Builder 方式设置初始选中节点；ID 不存在时保持未选中。
    pub fn selected(mut self, id: TreeNodeId) -> Self {
        self.select(id);
        self
    }

    pub fn selected_node(&self) -> Option<&TreeNode> {
        self.selection.and_then(|id| self.find_node(id))
    }

    pub fn find_node(&self, id: TreeNodeId) -> Option<&TreeNode> {
        find_node(&self.nodes, id)
    }

    pub fn find_node_mut(&mut self, id: TreeNodeId) -> Option<&mut TreeNode> {
        find_node_mut(&mut self.nodes, id)
    }

    /// 选择指定节点。隐藏节点也可被选择，但只有可见节点会触发滚动跟随。
    pub fn select(&mut self, id: TreeNodeId) -> bool {
        if self.find_node(id).is_none() {
            return false;
        }
        let changed = self.selection != Some(id);
        self.selection = Some(id);
        self.ensure_node_visible(id);
        changed
    }

    pub fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    pub fn set_expanded(&mut self, id: TreeNodeId, expanded: bool) -> bool {
        let changed = self
            .find_node_mut(id)
            .is_some_and(|node| node.set_expanded(expanded));
        if changed {
            self.refresh_metrics();
        }
        changed
    }

    pub fn toggle_expanded(&mut self, id: TreeNodeId) -> bool {
        let Some(node) = self.find_node_mut(id) else {
            return false;
        };
        if node.children.is_empty() {
            return false;
        }
        node.expanded = !node.expanded;
        self.refresh_metrics();
        true
    }

    pub fn set_checked(&mut self, id: TreeNodeId, checked: bool) -> bool {
        self.find_node_mut(id)
            .is_some_and(|node| node.set_checked(checked))
    }

    pub fn toggle_checked(&mut self, id: TreeNodeId) -> bool {
        let show_checkboxes = self.show_checkboxes;
        let Some(node) = self.find_node_mut(id) else {
            return false;
        };
        if !show_checkboxes && !node.is_checkable() {
            return false;
        }
        node.checked = Some(!node.checked.unwrap_or(false));
        true
    }

    /// 展开所有分支节点。
    pub fn expand_all(&mut self) {
        visit_nodes_mut(&mut self.nodes, &mut |node| {
            node.expanded = !node.children.is_empty();
        });
        self.refresh_metrics();
    }

    /// 收起所有分支节点；选中节点仍保留，可再次展开后恢复显示。
    pub fn collapse_all(&mut self) {
        visit_nodes_mut(&mut self.nodes, &mut |node| node.expanded = false);
        self.refresh_metrics();
    }

    fn visible_rows(&self) -> Vec<VisibleRow> {
        let mut rows = Vec::new();
        collect_visible_rows(&self.nodes, 0, &mut Vec::new(), &mut rows);
        rows
    }

    fn node_by_path(&self, path: &[usize]) -> Option<&TreeNode> {
        node_by_path(&self.nodes, path)
    }

    fn visible_index_of(&self, id: TreeNodeId) -> Option<usize> {
        self.visible_rows().iter().position(|row| row.id == id)
    }

    fn all_node_ids(&self) -> Vec<TreeNodeId> {
        let mut ids = Vec::new();
        collect_node_ids(&self.nodes, &mut ids);
        ids
    }

    fn repair_selection(&mut self) {
        if self
            .selection
            .is_some_and(|id| self.find_node(id).is_none())
        {
            self.selection = None;
        }
    }

    fn refresh_metrics(&mut self) {
        let viewport = layout::content_rect(&self.base).size;
        let total_height = self.visible_rows().len() as f32 * self.row_height;
        self.scroll
            .set_metrics(Size::new(viewport.width, total_height), viewport);
    }

    fn ensure_node_visible(&mut self, id: TreeNodeId) {
        let Some(index) = self.visible_index_of(id) else {
            return;
        };
        let row = Rect::new(
            0.0,
            index as f32 * self.row_height,
            self.scroll.viewport().width,
            self.row_height,
        );
        self.scroll.ensure_visible(row, 0.0);
    }

    fn scrollbar_viewport(&self) -> Rect {
        let content = layout::content_rect(&self.base);
        Rect::new(
            content.left(),
            content.top(),
            (self.base.rect.right() - content.left()).max(0.0),
            content.size.height,
        )
    }

    fn select_relative(&mut self, offset: isize) -> bool {
        let rows = self.visible_rows();
        if rows.is_empty() {
            return false;
        }
        let current = self
            .selection
            .and_then(|id| rows.iter().position(|row| row.id == id));
        let next = match current {
            Some(index) => (index as isize + offset).clamp(0, rows.len() as isize - 1) as usize,
            None if offset < 0 => rows.len() - 1,
            None => 0,
        };
        self.select(rows[next].id)
    }

    fn handle_left_key(&mut self) -> bool {
        let Some(selected) = self.selection else {
            return self.select_relative(1);
        };
        let rows = self.visible_rows();
        let Some(row) = rows.iter().find(|row| row.id == selected) else {
            return false;
        };
        let path = row.path.clone();
        if self
            .find_node(selected)
            .is_some_and(|node| node.expanded && !node.children.is_empty())
        {
            return self.set_expanded(selected, false);
        }
        if path.len() <= 1 {
            return false;
        }
        let parent = node_by_path(&self.nodes, &path[..path.len() - 1]).map(TreeNode::id);
        parent.is_some_and(|id| self.select(id))
    }

    fn handle_right_key(&mut self) -> bool {
        let Some(selected) = self.selection else {
            return self.select_relative(1);
        };
        let Some(node) = self.find_node(selected) else {
            return false;
        };
        if node.children.is_empty() {
            return false;
        }
        if !node.expanded {
            return self.set_expanded(selected, true);
        }
        let first_child = node.children[0].id;
        self.select(first_child)
    }

    fn handle_pointer_down(&mut self, pos: Point) -> EventFlow {
        let content = layout::content_rect(&self.base);
        if !content.contains(pos) {
            return EventFlow::Ignored;
        }
        let content_y = pos.y - content.top() + self.scroll.offset().y;
        if content_y < 0.0 {
            return EventFlow::Ignored;
        }
        let row_index = (content_y / self.row_height) as usize;
        let Some(row) = self.visible_rows().get(row_index).cloned() else {
            return EventFlow::Ignored;
        };
        let row_left = content.left() + row.depth as f32 * self.indent;
        let row_top = content.top() + row_index as f32 * self.row_height - self.scroll.offset().y;
        let disclosure = Rect::new(row_left, row_top, INDICATOR_SLOT, self.row_height);
        let node_has_children = self
            .node_by_path(&row.path)
            .is_some_and(|node| !node.children.is_empty());
        if node_has_children && disclosure.contains(pos) {
            self.toggle_expanded(row.id);
            return EventFlow::Consumed;
        }

        let checkable = self
            .node_by_path(&row.path)
            .is_some_and(TreeNode::is_checkable)
            || self.show_checkboxes;
        let checkbox = Rect::new(
            row_left + INDICATOR_SLOT,
            disclosure.top(),
            INDICATOR_SLOT,
            self.row_height,
        );
        self.selection = Some(row.id);
        if checkable && checkbox.contains(pos) {
            self.toggle_checked(row.id);
        }
        self.ensure_node_visible(row.id);
        EventFlow::Consumed
    }
}

impl Default for TreeView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TreeView {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let rows = self.visible_rows();
        let width = rows
            .iter()
            .filter_map(|row| {
                let node = self.node_by_path(&row.path)?;
                let controls = INDICATOR_SLOT
                    + if self.show_checkboxes || node.is_checkable() {
                        INDICATOR_SLOT
                    } else {
                        0.0
                    };
                Some(
                    row.depth as f32 * self.indent
                        + controls
                        + 8.0
                        + cv.measure_text(node.text(), &self.base.font).width,
                )
            })
            .fold(160.0, f32::max);
        layout::size_from_content(&self.base, width, rows.len() as f32 * self.row_height)
    }

    fn arrange(&mut self, content: Rect, _cv: &dyn Canvas) {
        let total_height = self.visible_rows().len() as f32 * self.row_height;
        self.scroll
            .set_metrics(Size::new(content.size.width, total_height), content.size);
    }

    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        if content.size.width <= 0.0 || content.size.height <= 0.0 {
            return;
        }
        let foreground = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
        let accent = style.accent_color.unwrap_or(foreground);
        let selection = style.selection_color.unwrap_or(DEFAULT_SELECTION_COLOR);
        let scroll_y = self.scroll.offset().y;

        cv.save();
        cv.clip_rect(content);
        for (index, row) in self.visible_rows().iter().enumerate() {
            let y = content.top() + index as f32 * self.row_height - scroll_y;
            if y + self.row_height < content.top() || y > content.bottom() {
                continue;
            }
            let Some(node) = self.node_by_path(&row.path) else {
                continue;
            };
            let row_rect = Rect::new(content.left(), y, content.size.width, self.row_height);
            if self.selection == Some(node.id) {
                cv.fill_rect(row_rect, selection);
            }

            let mut cursor = content.left() + row.depth as f32 * self.indent;
            if !node.children.is_empty() {
                paint_disclosure(
                    cv,
                    Rect::new(cursor, y, INDICATOR_SLOT, self.row_height),
                    node.expanded,
                    foreground,
                );
            }
            cursor += INDICATOR_SLOT;

            if self.show_checkboxes || node.is_checkable() {
                paint_checkbox(
                    cv,
                    Rect::new(cursor, y, INDICATOR_SLOT, self.row_height),
                    node.checked.unwrap_or(false),
                    foreground,
                    accent,
                );
                cursor += INDICATOR_SLOT;
            }

            let text_rect = Rect::new(
                cursor + 4.0,
                y,
                (content.right() - cursor - 4.0).max(0.0),
                self.row_height,
            );
            draw_aligned_text(
                cv,
                node.text(),
                text_rect,
                &self.base.font,
                foreground,
                TextAlign::Left,
                true,
            );
        }
        cv.restore();
        paint_scrollbars(
            cv,
            self.scrollbar_viewport(),
            &self.scroll,
            &self.scrollbar,
            style,
        );
    }

    fn on_event(&mut self, event: &Event) -> EventFlow {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => self.handle_pointer_down(*pos),
            Event::KeyDown { key: keys::UP, .. } => {
                self.select_relative(-1);
                EventFlow::Consumed
            }
            Event::KeyDown {
                key: keys::DOWN, ..
            } => {
                self.select_relative(1);
                EventFlow::Consumed
            }
            Event::KeyDown {
                key: keys::LEFT, ..
            } => {
                self.handle_left_key();
                EventFlow::Consumed
            }
            Event::KeyDown {
                key: keys::RIGHT, ..
            } => {
                self.handle_right_key();
                EventFlow::Consumed
            }
            Event::KeyDown {
                key: keys::ENTER, ..
            } => {
                if let Some(id) = self.selection {
                    self.toggle_expanded(id);
                }
                EventFlow::Consumed
            }
            Event::KeyDown { key: 32, .. } => {
                if let Some(id) = self.selection {
                    self.toggle_checked(id);
                }
                EventFlow::Consumed
            }
            _ => EventFlow::Ignored,
        }
    }

    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Items(items) => {
                self.nodes = items.into_iter().map(TreeNode::new).collect();
                self.repair_selection();
                self.refresh_metrics();
                true
            }
            WidgetProperty::SelectedIndex(index) => self.set_selected_index(index),
            WidgetProperty::RowHeight(height) => {
                self.row_height = height.max(1.0);
                self.refresh_metrics();
                true
            }
            WidgetProperty::ScrollBar(visibility) => {
                self.scroll.set_visibility(visibility);
                true
            }
            _ => false,
        }
    }

    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        match key {
            WidgetPropertyKey::Items => Some(WidgetProperty::Items(
                self.nodes.iter().map(|node| node.text.clone()).collect(),
            )),
            WidgetPropertyKey::SelectedIndex => {
                self.selected_index().map(WidgetProperty::SelectedIndex)
            }
            WidgetPropertyKey::RowHeight => Some(WidgetProperty::RowHeight(self.row_height)),
            WidgetPropertyKey::ScrollBar => {
                Some(WidgetProperty::ScrollBar(self.scroll.visibility()))
            }
            _ => None,
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.selection.and_then(|id| {
            self.all_node_ids()
                .iter()
                .position(|candidate| *candidate == id)
        })
    }

    fn set_selected_index(&mut self, index: usize) -> bool {
        self.all_node_ids()
            .get(index)
            .copied()
            .is_some_and(|id| self.select(id))
    }

    fn is_scrollable(&self) -> bool {
        true
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.scroll.scroll_by(dx, dy)
    }

    fn scroll_offset(&self) -> Option<Point> {
        Some(self.scroll.offset())
    }

    fn scrollbar_grab(&self, pos: Point) -> Option<crate::scroll::ScrollGrab> {
        crate::scroll::thumb_grab(
            &self.scroll,
            self.scrollbar_viewport(),
            &self.scrollbar,
            pos,
        )
    }

    fn scrollbar_drag(&mut self, pos: Point, grab: &crate::scroll::ScrollGrab) -> bool {
        let viewport = self.scrollbar_viewport();
        crate::scroll::apply_thumb_drag(&mut self.scroll, viewport, &self.scrollbar, pos, grab)
    }

    fn scrollbar_contains(&self, pos: Point) -> bool {
        crate::scroll::scrollbar_region_contains(
            &self.scroll,
            self.scrollbar_viewport(),
            &self.scrollbar,
            pos,
        )
    }

    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        self.scroll.axis_value(prop)
    }

    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        self.scroll.set_axis_value(prop, value)
    }
}

common_builders!(TreeView);

fn collect_visible_rows(
    nodes: &[TreeNode],
    depth: usize,
    path: &mut Vec<usize>,
    rows: &mut Vec<VisibleRow>,
) {
    for (index, node) in nodes.iter().enumerate() {
        path.push(index);
        rows.push(VisibleRow {
            id: node.id,
            depth,
            path: path.clone(),
        });
        if node.expanded {
            collect_visible_rows(&node.children, depth + 1, path, rows);
        }
        path.pop();
    }
}

fn find_node(nodes: &[TreeNode], id: TreeNodeId) -> Option<&TreeNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, id) {
            return Some(found);
        }
    }
    None
}

fn collect_node_ids(nodes: &[TreeNode], ids: &mut Vec<TreeNodeId>) {
    for node in nodes {
        ids.push(node.id);
        collect_node_ids(&node.children, ids);
    }
}

fn find_node_mut(nodes: &mut [TreeNode], id: TreeNodeId) -> Option<&mut TreeNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

fn remove_node(nodes: &mut Vec<TreeNode>, id: TreeNodeId) -> Option<TreeNode> {
    if let Some(index) = nodes.iter().position(|node| node.id == id) {
        return Some(nodes.remove(index));
    }
    for node in nodes {
        if let Some(removed) = remove_node(&mut node.children, id) {
            return Some(removed);
        }
    }
    None
}

fn node_by_path<'a>(nodes: &'a [TreeNode], path: &[usize]) -> Option<&'a TreeNode> {
    let (first, rest) = path.split_first()?;
    let node = nodes.get(*first)?;
    if rest.is_empty() {
        Some(node)
    } else {
        node_by_path(&node.children, rest)
    }
}

fn visit_nodes_mut(nodes: &mut [TreeNode], visitor: &mut impl FnMut(&mut TreeNode)) {
    for node in nodes {
        visitor(node);
        visit_nodes_mut(&mut node.children, visitor);
    }
}

fn paint_disclosure(cv: &mut dyn Canvas, slot: Rect, expanded: bool, color: Color) {
    let center_x = slot.left() + slot.size.width / 2.0;
    let center_y = slot.top() + slot.size.height / 2.0;
    if expanded {
        // 以细矩形逐行拼出向下三角，避免给 Canvas 增加路径 API。
        for row in 0..4 {
            let half = 4.0 - row as f32;
            cv.fill_rect(
                Rect::new(
                    center_x - half,
                    center_y - 2.0 + row as f32,
                    half * 2.0,
                    1.0,
                ),
                color,
            );
        }
    } else {
        // 向右三角。
        for row in -4_i32..=4 {
            let width = 5.0 - row.unsigned_abs() as f32;
            cv.fill_rect(
                Rect::new(center_x - 2.0, center_y + row as f32, width, 1.0),
                color,
            );
        }
    }
}

fn paint_checkbox(cv: &mut dyn Canvas, slot: Rect, checked: bool, border: Color, accent: Color) {
    let size = slot.size.height.min(14.0).min(slot.size.width);
    let rect = Rect::new(
        slot.left() + (slot.size.width - size) / 2.0,
        slot.top() + (slot.size.height - size) / 2.0,
        size,
        size,
    );
    cv.stroke_rect(rect, border, 1.0);
    if checked {
        cv.fill_rect(rect.deflate(flexui_gfx::Insets::all(3.0)), accent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Mods;

    fn sample_tree() -> TreeView {
        TreeView::new().nodes([
            TreeNode::with_id(1, "src")
                .expanded(true)
                .checked(true)
                .child(TreeNode::with_id(2, "widgets").child(TreeNode::with_id(3, "tree.rs")))
                .child(TreeNode::with_id(4, "lib.rs")),
            TreeNode::with_id(5, "Cargo.toml"),
        ])
    }

    #[test]
    fn 可见行随展开状态变化() {
        let mut tree = sample_tree();
        assert_eq!(
            tree.visible_rows()
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [1, 2, 4, 5]
        );

        assert!(tree.set_expanded(2, true));
        assert_eq!(
            tree.visible_rows()
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        assert!(tree.set_expanded(1, false));
        assert_eq!(
            tree.visible_rows()
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [1, 5]
        );

        // 通用 SelectedIndex 使用完整树的前序索引，父节点收起时仍保持稳定。
        assert!(tree.set_selected_index(2));
        assert_eq!(tree.selection(), Some(3));
        assert_eq!(tree.selected_index(), Some(2));
    }

    #[test]
    fn 点击箭头复选框和文本区分别生效() {
        let mut tree = sample_tree();
        tree.base_mut().rect = Rect::new(0.0, 0.0, 180.0, 120.0);
        tree.arrange(Rect::new(0.0, 0.0, 180.0, 120.0), &FakeCanvas);

        // 第一行箭头区：只收起，不改变选择。
        tree.on_event(&mouse_down(10.0, 10.0));
        assert!(!tree.find_node(1).unwrap().is_expanded());
        assert_eq!(tree.selection(), None);

        // 再展开，然后点击复选框槽：选中节点并取消勾选。
        tree.on_event(&mouse_down(10.0, 10.0));
        tree.on_event(&mouse_down(30.0, 10.0));
        assert_eq!(tree.selection(), Some(1));
        assert_eq!(tree.find_node(1).unwrap().is_checked(), Some(false));

        // 第二行文本区选中子节点。
        tree.on_event(&mouse_down(70.0, 38.0));
        assert_eq!(tree.selection(), Some(2));
    }

    #[test]
    fn 键盘导航与滚动跟随可用() {
        let nodes = (0..8).map(|id| TreeNode::with_id(id + 1, format!("node-{id}")));
        let mut tree = TreeView::new().nodes(nodes).row_height(20.0);
        tree.base_mut().rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        tree.arrange(Rect::new(0.0, 0.0, 100.0, 40.0), &FakeCanvas);

        assert!(tree.set_selected_index(7));
        assert_eq!(tree.selection(), Some(8));
        assert_eq!(tree.scroll_offset(), Some(Point::new(0.0, 120.0)));
        tree.on_event(&Event::KeyDown {
            key: keys::UP,
            mods: Mods::default(),
        });
        assert_eq!(tree.selection(), Some(7));
    }

    #[test]
    fn 纯代码接口支持增删查改和全局复选框() {
        let mut tree = TreeView::new()
            .checkboxes(true)
            .node(TreeNode::with_id(10, "root").child(TreeNode::with_id(11, "child")));
        tree.add_node(TreeNode::with_id(20, "other"));
        tree.find_node_mut(11).unwrap().set_text("renamed");
        assert_eq!(tree.find_node(11).unwrap().text(), "renamed");
        assert!(tree.add_child(10, TreeNode::with_id(12, "added")));
        assert!(tree.toggle_checked(20));
        assert_eq!(tree.find_node(20).unwrap().is_checked(), Some(true));
        assert_eq!(tree.remove_node(12).map(|node| node.id()), Some(12));
        assert!(tree.find_node(12).is_none());
        tree.expand_all();
        assert!(tree.find_node(10).unwrap().is_expanded());
        tree.collapse_all();
        assert!(!tree.find_node(10).unwrap().is_expanded());
    }

    fn mouse_down(x: f32, y: f32) -> Event {
        Event::MouseDown {
            pos: Point::new(x, y),
            button: MouseButton::Left,
            mods: Mods::default(),
        }
    }

    struct FakeCanvas;

    impl Canvas for FakeCanvas {
        fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: Color, _line_width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: flexui_gfx::Corners, _color: Color) {}
        fn stroke_round_rect(
            &mut self,
            _rect: Rect,
            _radius: flexui_gfx::Corners,
            _color: Color,
            _line_width: f32,
        ) {
        }
        fn draw_text(
            &mut self,
            _text: &str,
            _origin: Point,
            _font: &flexui_gfx::Font,
            _color: Color,
        ) {
        }
        fn measure_text(&self, text: &str, font: &flexui_gfx::Font) -> Size {
            Size::new(
                text.chars().count() as f32 * font.size * 0.6,
                font.size * 1.2,
            )
        }
    }
}
