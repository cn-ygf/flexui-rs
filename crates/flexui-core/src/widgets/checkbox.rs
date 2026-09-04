//! CheckBox：勾选框（点击切换 selected）。

use flexui_gfx::Canvas;
use flexui_gfx::Size;

use crate::common_builders;
use crate::layout;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Clickable, TextControl, Widget, WidgetRole};

use super::paint_indicator_and_text;

/// 勾选框：点击切换 selected。
pub struct CheckBox {
    base: Base,
}

impl CheckBox {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new_kind(WidgetRole::CheckBox, WidgetKind::CheckBox);
        base.text = text.into();
        Self { base }
    }
    pub fn checked(mut self, v: bool) -> Self {
        self.base.selected = v;
        self
    }
}

impl Widget for CheckBox {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        layout::size_from_content(&self.base, s.width + 26.0, s.height.max(18.0))
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        paint_indicator_and_text(&self.base, cv, style, false);
    }
}

common_builders!(CheckBox);

impl TextControl for CheckBox {}
impl Clickable for CheckBox {}
