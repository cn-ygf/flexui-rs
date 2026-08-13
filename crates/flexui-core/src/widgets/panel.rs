//! Panel（Box）：通用容器——子控件叠放、各自填充内容区。
//! 既可当单子容器（放 1 个子），也可当 Box 多子容器（多个叠放）。

use crate::common_builders;
use crate::theme::WidgetKind;
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

/// 通用容器：子控件叠放、各自填充内容区。
pub struct Panel {
    base: Base,
}

impl Panel {
    pub fn new() -> Self {
        Self {
            base: Base::new_kind(WidgetRole::Plain, WidgetKind::Panel),
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

impl Container for Panel {}
