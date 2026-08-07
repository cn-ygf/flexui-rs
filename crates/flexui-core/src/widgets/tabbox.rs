//! TabBox：多页容器（仅显示 selected_index 指向的那一页，配合 Radio 组成 tabbar）。

use flexui_geometry::{Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout;
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

/// 多页容器：仅显示 `selected_index` 指向的那一页，配合 Radio 组成 tabbar。
pub struct TabBox {
    base: Base,
}

impl TabBox {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::TabBox),
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
        self.base.selected_index = i;
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
        // 取各页最大尺寸，保证切换时容器尺寸稳定。
        layout::measure_stack(&mut self.base, avail, cv)
    }
    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        let sel = self.base.selected_index;
        for (i, child) in self.base.children.iter_mut().enumerate() {
            // 只让当前页可见，其余页隐藏（隐藏页跳过绘制与命中）。
            child.base_mut().visible = i == sel;
            layout::layout_node(child.as_mut(), content, cv);
        }
    }
}

common_builders!(TabBox);

impl Container for TabBox {}
