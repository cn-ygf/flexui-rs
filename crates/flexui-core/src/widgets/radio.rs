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
    indicator_visible: bool,
}

impl Radio {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new(WidgetRole::Radio);
        base.text = text.into();
        Self { base, group: None, tab_index: None, indicator_visible: true }
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
    /// 是否绘制左侧单选指示器；关闭后可用作纯文字 Tab。
    pub fn indicator_visible(mut self, visible: bool) -> Self {
        self.indicator_visible = visible;
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
        if self.indicator_visible {
            paint_indicator_and_text(&self.base, cv, style, true);
        } else {
            let color = style.fg_color.unwrap_or(flexui_geometry::Color::WHITE);
            let align = style.text_align.unwrap_or(flexui_gfx::TextAlign::Center);
            crate::paint::draw_aligned_text(
                cv,
                &self.base.text,
                crate::layout::content_rect(&self.base),
                &self.base.font,
                color,
                align,
                true,
            );
        }
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Group(v) => self.group = v,
            WidgetProperty::TabIndex(v) => self.tab_index = v,
            WidgetProperty::IndicatorVisible(v) => self.indicator_visible = v,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_can_be_hidden_for_text_tabs() {
        let mut radio = Radio::new("Tab");
        assert!(radio.indicator_visible);
        assert!(radio.apply_property(WidgetProperty::IndicatorVisible(false)));
        assert!(!radio.indicator_visible);
    }
}
