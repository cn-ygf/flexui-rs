//! 容器控件（L4）：Panel（单子/叠放）、VBox、HBox、TabBox。对应需求 C4/C5/C9。

use flexui_geometry::{Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout::{self, Axis};
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

/// 通用容器：子控件叠放、各自填充内容区。既可当单子容器（放 1 个子），
/// 也可当 Box 多子容器（多个叠放）。
pub struct Panel {
    base: Base,
}

impl Panel {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Plain),
        }
    }
    /// 追加一个子控件。
    pub fn push(mut self, child: impl Widget + 'static) -> Self {
        self.base.children.push(Box::new(child));
        self
    }
    /// 追加一个已装箱的子控件（供 XML 构建用）。
    pub fn push_node(mut self, child: Node) -> Self {
        self.base.children.push(child);
        self
    }
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Panel {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
}

common_builders!(Panel);

/// 纵向弹性容器。
pub struct VBox {
    base: Base,
}

impl VBox {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Plain),
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

/// 横向弹性容器。
pub struct HBox {
    base: Base,
}

impl HBox {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Plain),
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

// —— 容器能力 trait（继承视图）——
impl Container for Panel {}
impl Container for VBox {}
impl Container for HBox {}
impl Container for TabBox {}
