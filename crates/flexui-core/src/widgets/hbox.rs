//! HBox：横向弹性容器（主轴横向 Flex + flex_grow，交叉轴拉伸）。

use flexui_geometry::{Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout::{self, Axis};
use crate::theme::WidgetKind;
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

/// 横向弹性容器。
pub struct HBox {
    base: Base,
}

impl HBox {
    pub fn new() -> Self {
        Self {
            base: Base::new_kind(WidgetRole::Plain, WidgetKind::HBox),
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

impl Default for HBox {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for HBox {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, avail: Size, cv: &dyn Canvas) -> Size {
        layout::measure_axis(&mut self.base, Axis::Horizontal, avail, cv)
    }
    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        layout::arrange_axis(&mut self.base, Axis::Horizontal, content, cv)
    }
}

common_builders!(HBox);

impl Container for HBox {}
