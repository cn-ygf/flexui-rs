//! CheckBox：勾选框（点击切换 selected）。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout;
use crate::style::StyleSpec;
use crate::widget::{Base, Clickable, TextControl, Widget, WidgetProperty, WidgetRole};

use super::paint_indicator_and_text;

/// 勾选框：点击切换 selected。
pub struct CheckBox {
    base: Base,
    switch_style: bool,
}

impl CheckBox {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new(WidgetRole::CheckBox);
        base.text = text.into();
        Self { base, switch_style: false }
    }
    pub fn checked(mut self, v: bool) -> Self {
        self.base.selected = v;
        self
    }
    /// 使用横向开关外观；默认仍为传统勾选框。
    pub fn switch(mut self, v: bool) -> Self {
        self.switch_style = v;
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
        if self.switch_style && style.bg_image.is_none() && style.fg_image.is_none() {
            paint_switch_knob(&self.base, cv, style);
        } else {
            paint_indicator_and_text(&self.base, cv, style, false);
        }
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        if let WidgetProperty::SwitchStyle(v) = property { self.switch_style = v; true } else { false }
    }
}

/// 开关轨道由通用背景样式绘制，这里只画可移动圆点。
fn paint_switch_knob(base: &Base, cv: &mut dyn Canvas, style: &StyleSpec) {
    let content = layout::content_rect(base);
    let diameter = (content.size.height - 8.0).max(8.0);
    let x = if base.selected {
        content.right() - diameter - 4.0
    } else {
        content.left() + 4.0
    };
    let knob = Rect::new(
        x,
        content.top() + (content.size.height - diameter) / 2.0,
        diameter,
        diameter,
    );
    cv.fill_round_rect(
        knob,
        Corners::all(diameter / 2.0),
        style
            .fg_color
            .unwrap_or_else(|| Color::from_u8(255, 255, 255, 255)),
    );
}

common_builders!(CheckBox);

impl TextControl for CheckBox {}
impl Clickable for CheckBox {}
