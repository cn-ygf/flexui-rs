//! TabBox：多页容器（仅显示 selected_index 指向的那一页，配合 Radio 组成 tabbar）。

use flexui_geometry::{Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout;
use crate::widget::{Base, Container, Node, Widget, WidgetProperty, WidgetRole};

/// 多页容器：仅显示 `selected_index` 指向的那一页，配合 Radio 组成 tabbar。
pub struct TabBox {
    base: Base,
    selected_index: usize,
}

impl TabBox {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::TabBox),
            selected_index: 0,
        }
    }
    /// 追加一页。
    pub fn page(mut self, child: impl Widget + 'static) -> Self {
        self.base.children.push(Box::new(child));
        self
    }
    pub fn page_node(mut self, child: Node) -> Self {
        self.base.children.push(child);
        self
    }
    /// 初始选中页。
    pub fn selected(mut self, i: usize) -> Self {
        self.selected_index = i;
        self
    }
}

impl Default for TabBox {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TabBox {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, avail: Size, cv: &dyn Canvas) -> Size {
        // 隐藏页也参与度量，保证切换页面时容器尺寸稳定。
        let inner = Size::new(
            (avail.width - self.base.padding.horizontal()).max(0.0),
            (avail.height - self.base.padding.vertical()).max(0.0),
        );
        let mut width = 0.0f32;
        let mut height = 0.0f32;
        for child in self.base.children.iter_mut() {
            let was_visible = child.base().visible;
            child.base_mut().visible = true;
            let size = layout::measure_node(child.as_mut(), inner, cv);
            child.base_mut().visible = was_visible;
            let margin = child.base().margin;
            width = width.max(size.width + margin.horizontal());
            height = height.max(size.height + margin.vertical());
        }
        layout::size_from_content(&self.base, width, height)
    }
    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        let sel = self.selected_index;
        for (i, child) in self.base.children.iter_mut().enumerate() {
            // 只让当前页可见，其余页隐藏（隐藏页跳过绘制与命中）。
            child.base_mut().visible = i == sel;
            if i == sel {
                layout::layout_node(child.as_mut(), content, cv);
            }
        }
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        if let WidgetProperty::SelectedIndex(i) = property { self.selected_index = i; true } else { false }
    }
    fn selected_index(&self) -> Option<usize> { Some(self.selected_index) }
    fn set_selected_index(&mut self, index: usize) -> bool {
        self.selected_index = index.min(self.base.children.len().saturating_sub(1));
        true
    }
}

common_builders!(TabBox);

impl Container for TabBox {}
