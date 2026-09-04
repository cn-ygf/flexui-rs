//! HBox：横向弹性容器（主轴横向 Flex + flex_grow，交叉轴拉伸）。

use flexui_gfx::Canvas;
use flexui_gfx::{Rect, Size};

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
    /// 设置初始选中态（参与样式解析的 selected 维度）。
    /// 用于可点击富行等：切换选中只需 `ctx.set_selected(name, bool)`，无需重建。
    pub fn selected(mut self, on: bool) -> Self {
        self.base.selected = on;
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
