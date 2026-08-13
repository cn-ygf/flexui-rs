//! VBox：纵向弹性容器（主轴纵向 Flex + flex_grow，交叉轴拉伸）。

use flexui_geometry::{Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout::{self, Axis};
use crate::theme::WidgetKind;
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

/// 纵向弹性容器。
pub struct VBox {
    base: Base,
}

impl VBox {
    pub fn new() -> Self {
        Self {
            base: Base::new_kind(WidgetRole::Plain, WidgetKind::VBox),
        }
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.base.spacing = s;
        self
    }
    pub fn push(mut self, child: impl Widget + 'static) -> Self {
        self.base.children.push(Box::new(child));
        self
    }
    pub fn push_node(mut self, child: Node) -> Self {
        self.base.children.push(child);
        self
    }
}

impl Default for VBox {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for VBox {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, avail: Size, cv: &dyn Canvas) -> Size {
        layout::measure_axis(&mut self.base, Axis::Vertical, avail, cv)
    }
    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        layout::arrange_axis(&mut self.base, Axis::Vertical, content, cv)
    }
}

common_builders!(VBox);

impl Container for VBox {}
