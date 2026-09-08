//! ListView：同时支持轻量文本行与任意复杂控件行。
//!
//! 文本行由 ListView 直接绘制，适合大量简单数据；复杂行作为普通 Widget 子树参与
//! 测量、布局、绘制与事件分发，可由纯 Rust 构造，也可由 XML 条目模板生成。

use flexui_gfx::{Canvas, Color, Point, Rect, Size, TextAlign};

use crate::anim::AnimProp;
use crate::common_builders;
use crate::event::{Event, EventFlow, MouseButton};
use crate::layout::{self, layout_node, measure_node};
use crate::paint::draw_aligned_text;
use crate::scroll::{paint_scrollbars, ScrollAxes, ScrollBarStyle, ScrollState};
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Node, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole};

/// 选中行高亮色。
const SEL_COLOR: Color = Color::rgba(0.20, 0.45, 0.95, 0.45);

/// 列表行的存储形式。复杂行记录对应的 Widget 子节点下标。
enum ListEntry {
    Text(String),
    Control { child_index: usize },
}

/// 可滚动列表。文本行走轻量自绘路径，复杂行使用完整控件树。
pub struct ListView {
    base: Base,
    entries: Vec<ListEntry>,
    row_heights: Vec<f32>,
    row_h: f32,
    selection: Option<usize>,
    scroll: ScrollState,
    scrollbar: ScrollBarStyle,
}

impl ListView {
    pub fn new() -> Self {
        Self {
            base: Base::new_kind(WidgetRole::ListView, WidgetKind::ListView),
            entries: Vec::new(),
            row_heights: Vec::new(),
            row_h: 28.0,
            selection: None,
            scroll: ScrollState::new(ScrollAxes::vertical()),
            scrollbar: ScrollBarStyle::default(),
        }
    }

    /// 兼容旧 API：替换成一组轻量文本行。
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.set_text_items(items);
        self
    }

    /// 追加一行轻量文本。
    pub fn text_item(mut self, text: impl Into<String>) -> Self {
        self.push_text(text);
        self
    }

    /// 追加一行复杂控件。
    pub fn item(mut self, child: impl Widget + 'static) -> Self {
        self.push_item(Box::new(child));
        self
    }

    /// 运行时追加轻量文本行。
    pub fn push_text(&mut self, text: impl Into<String>) {
        self.entries.push(ListEntry::Text(text.into()));
        self.row_heights.push(self.row_h);
    }

    /// 运行时追加复杂控件行。
    pub fn push_item(&mut self, child: Node) {
        let child_index = self.base.children.len();
        self.base.children.push(child);
        self.entries.push(ListEntry::Control { child_index });
        self.row_heights.push(self.row_h);
    }

    /// 替换为一组轻量文本行，同时清空原有复杂行子树。
    pub fn set_text_items(&mut self, items: impl IntoIterator<Item = impl Into<String>>) {
        self.base.children.clear();
        self.entries = items
            .into_iter()
            .map(|item| ListEntry::Text(item.into()))
            .collect();
        self.row_heights = vec![self.row_h; self.entries.len()];
        self.selection = self.selection.filter(|index| *index < self.entries.len());
    }

    pub fn item_count(&self) -> usize {
        self.entries.len()
    }

    /// 所有行的最小行高。复杂控件测量高度更大时会自动撑高对应行。
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_h = height.max(1.0);
        self.row_heights.fill(self.row_h);
        self
    }

    /// 滚动条可见性模式（auto/always/hidden）。
    pub fn scrollbar(mut self, visibility: crate::scroll::ScrollBarVisibility) -> Self {
        self.scroll.set_visibility(visibility);
        self
    }

    /// 预选中第 i 行。
    pub fn selected(mut self, index: usize) -> Self {
        if index < self.entries.len() {
            self.selection = Some(index);
        }
        self
    }

    pub fn selection(&self) -> Option<usize> {
        self.selection
    }

    fn content_viewport(&self) -> Rect {
        layout::content_rect(&self.base)
    }

    /// 滚动条所用视口：纵向跟随内容区，横向贴控件外缘。
    fn scrollbar_viewport(&self) -> Rect {
        let content = self.content_viewport();
        Rect::new(
            content.left(),
            content.top(),
            (self.base.rect.right() - content.left()).max(0.0),
            content.size.height,
        )
    }

    fn total_height(&self) -> f32 {
        self.row_heights.iter().sum()
    }

    fn row_top(&self, index: usize) -> f32 {
        self.row_heights.iter().take(index).sum()
    }

    fn row_at_offset(&self, offset: f32) -> Option<usize> {
        if offset < 0.0 {
            return None;
        }
        let mut top = 0.0;
        for (index, height) in self.row_heights.iter().copied().enumerate() {
            if offset < top + height {
                return Some(index);
            }
            top += height;
        }
        None
    }

    /// 若已知视口，滚动使第 i 行进入可见区。
    fn ensure_row_visible(&mut self, index: usize) {
        let row = Rect::new(
            0.0,
            self.row_top(index),
            self.scroll.viewport().width,
            self.row_heights.get(index).copied().unwrap_or(self.row_h),
        );
        let before = self.scroll.offset();
        self.scroll.ensure_visible(row, 0.0);
        self.shift_children(before);
    }

    /// 滚动后立即平移复杂行子树，无需等待下一次布局。
    fn shift_children(&mut self, before: Point) {
        let after = self.scroll.offset();
        let dx = before.x - after.x;
        let dy = before.y - after.y;
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        for child in &mut self.base.children {
            translate_subtree(child.as_mut(), dx, dy);
        }
    }
}

impl Default for ListView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ListView {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn measure(&mut self, avail: Size, cv: &dyn Canvas) -> Size {
        let inner_width = avail.width.max(0.0);
        self.row_heights.resize(self.entries.len(), self.row_h);
        for (index, entry) in self.entries.iter().enumerate() {
            self.row_heights[index] = match entry {
                ListEntry::Text(_) => self.row_h,
                ListEntry::Control { child_index } => self
                    .base
                    .children
                    .get_mut(*child_index)
                    .map(|child| {
                        measure_node(child.as_mut(), Size::new(inner_width, avail.height), cv)
                            .height
                            .max(self.row_h)
                    })
                    .unwrap_or(self.row_h),
            };
        }
        layout::size_from_content(&self.base, 160.0, self.total_height())
    }

    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        self.row_heights.resize(self.entries.len(), self.row_h);
        for (index, entry) in self.entries.iter().enumerate() {
            if let ListEntry::Control { child_index } = entry {
                if let Some(child) = self.base.children.get_mut(*child_index) {
                    self.row_heights[index] = measure_node(child.as_mut(), content.size, cv)
                        .height
                        .max(self.row_h);
                }
            }
        }
        self.scroll.set_metrics(
            Size::new(content.size.width, self.total_height()),
            content.size,
        );

        let mut y = content.top() - self.scroll.offset().y;
        for (index, entry) in self.entries.iter().enumerate() {
            let height = self.row_heights[index];
            if let ListEntry::Control { child_index } = entry {
                if let Some(child) = self.base.children.get_mut(*child_index) {
                    layout_node(
                        child.as_mut(),
                        Rect::new(content.left(), y, content.size.width, height),
                        cv,
                    );
                }
            }
            y += height;
        }
    }

    fn children_viewport(&self) -> Rect {
        self.content_viewport()
    }

    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = self.content_viewport();
        if content.size.width <= 0.0 || content.size.height <= 0.0 {
            return;
        }
        let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
        let mut y = content.top() - self.scroll.offset().y;
        for (index, entry) in self.entries.iter().enumerate() {
            let height = self.row_heights.get(index).copied().unwrap_or(self.row_h);
            let row = Rect::new(content.left(), y, content.size.width, height);
            if row.bottom() >= content.top() && row.top() <= content.bottom() {
                if self.selection == Some(index) {
                    cv.fill_rect(row, style.selection_color.unwrap_or(SEL_COLOR));
                }
                if let ListEntry::Text(text) = entry {
                    let text_rect = Rect::new(
                        row.left() + 8.0,
                        row.top(),
                        (row.size.width - 8.0).max(0.0),
                        row.size.height,
                    );
                    draw_aligned_text(
                        cv,
                        text,
                        text_rect,
                        &self.base.font,
                        color,
                        TextAlign::Left,
                        true,
                    );
                }
            }
            y += height;
        }
    }

    fn paint_foreground(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        paint_scrollbars(
            cv,
            self.scrollbar_viewport(),
            &self.scroll,
            &self.scrollbar,
            style,
        );
    }

    fn on_event(&mut self, ev: &Event) -> EventFlow {
        if let Event::MouseDown {
            pos,
            button: MouseButton::Left,
            ..
        } = ev
        {
            let content = self.content_viewport();
            let offset = pos.y - content.top() + self.scroll.offset().y;
            if let Some(index) = self.row_at_offset(offset) {
                self.selection = Some(index);
            }
            return EventFlow::Consumed;
        }
        EventFlow::Ignored
    }

    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::SelectedIndex(index) => self.set_selected_index(index),
            WidgetProperty::Items(items) => {
                self.set_text_items(items);
                true
            }
            WidgetProperty::RowHeight(height) => {
                self.row_h = height.max(1.0);
                self.row_heights.fill(self.row_h);
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
                self.entries
                    .iter()
                    .filter_map(|entry| match entry {
                        ListEntry::Text(text) => Some(text.clone()),
                        ListEntry::Control { .. } => None,
                    })
                    .collect(),
            )),
            WidgetPropertyKey::SelectedIndex => self.selection.map(WidgetProperty::SelectedIndex),
            WidgetPropertyKey::RowHeight => Some(WidgetProperty::RowHeight(self.row_h)),
            WidgetPropertyKey::ScrollBar => {
                Some(WidgetProperty::ScrollBar(self.scroll.visibility()))
            }
            _ => None,
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.selection
    }

    fn set_selected_index(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        let changed = self.selection != Some(index);
        self.selection = Some(index);
        self.ensure_row_visible(index);
        changed
    }

    fn is_scrollable(&self) -> bool {
        true
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let before = self.scroll.offset();
        let changed = self.scroll.scroll_by(dx, dy);
        if changed {
            self.shift_children(before);
        }
        changed
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
        let before = self.scroll.offset();
        let viewport = self.scrollbar_viewport();
        let changed =
            crate::scroll::apply_thumb_drag(&mut self.scroll, viewport, &self.scrollbar, pos, grab);
        if changed {
            self.shift_children(before);
        }
        changed
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
        let before = self.scroll.offset();
        let handled = self.scroll.set_axis_value(prop, value);
        if handled {
            self.shift_children(before);
        }
        handled
    }
}

common_builders!(ListView);

fn translate_subtree(node: &mut dyn Widget, dx: f32, dy: f32) {
    node.base_mut().rect.origin.x += dx;
    node.base_mut().rect.origin.y += dy;
    let count = node.base().children.len();
    for index in 0..count {
        translate_subtree(node.base_mut().children[index].as_mut(), dx, dy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{Button, HBox, Label};

    #[test]
    fn listview_点击选中可变高度行() {
        let mut list = ListView::new()
            .items(["a", "b"])
            .item(HBox::new().height(44.0).push(Label::new("c")))
            .row_height(20.0);
        list.base_mut().rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        list.arrange(Rect::new(0.0, 0.0, 100.0, 60.0), &Fake);
        assert_eq!(list.scroll.content().height, 84.0);
        list.on_event(&Event::MouseDown {
            pos: Point::new(10.0, 50.0),
            button: MouseButton::Left,
            mods: crate::event::Mods::default(),
        });
        assert_eq!(list.selection(), Some(2));
    }

    #[test]
    fn listview_同时保存文本和复杂控件() {
        let list = ListView::new().text_item("plain").item(
            HBox::new()
                .push(Label::new("name"))
                .push(Button::new("action")),
        );
        assert_eq!(list.item_count(), 2);
        assert_eq!(list.base().children.len(), 1);
        let Some(WidgetProperty::Items(items)) = list.property(WidgetPropertyKey::Items) else {
            panic!("ListView 应返回文本条目属性");
        };
        assert_eq!(items, vec!["plain"]);
    }

    struct Fake;
    impl Canvas for Fake {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: flexui_gfx::Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: flexui_gfx::Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, _t: &str, _o: Point, _f: &flexui_gfx::Font, _c: Color) {}
        fn measure_text(&self, text: &str, font: &flexui_gfx::Font) -> Size {
            Size::new(
                text.chars().count() as f32 * font.size * 0.6,
                font.size * 1.2,
            )
        }
    }
}
