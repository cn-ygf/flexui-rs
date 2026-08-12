//! Radio：单选控件（同 group 互斥，可绑定 tab_index 驱动 TabBox 组成 tabbar）。

use flexui_geometry::Size;
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout;
use crate::style::StyleSpec;
use crate::widget::{Base, Clickable, TextControl, Widget, WidgetProperty, WidgetRole};

use super::paint_indicator_and_text;

/// 单选：同 group 互斥（互斥逻辑在分发器实现），可绑定 tab_index 驱动 TabBox。
pub struct Radio {
    base: Base,
    group: Option<u32>,
    tab_index: Option<usize>,
}

impl Radio {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new(WidgetRole::Radio);
        base.text = text.into();
        Self { base, group: None, tab_index: None }
    }
    /// 所属分组。
    pub fn group(mut self, g: u32) -> Self {
        self.group = Some(g);
        self
    }
    /// 关联的 TabBox 页索引（组成 tabbar）。
    pub fn tab_index(mut self, i: usize) -> Self {
        self.tab_index = Some(i);
        self
    }
    pub fn selected(mut self, v: bool) -> Self {
        self.base.selected = v;
        self
    }
}

impl Widget for Radio {
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
        paint_indicator_and_text(&self.base, cv, style, true);
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Group(v) => self.group = v,
            WidgetProperty::TabIndex(v) => self.tab_index = v,
            _ => return false,
        }
        true
    }
    fn selection_group(&self) -> Option<u32> { self.group }
    fn tab_index(&self) -> Option<usize> { self.tab_index }
}

common_builders!(Radio);

impl TextControl for Radio {}
impl Clickable for Radio {}
