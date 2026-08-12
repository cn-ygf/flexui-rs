//! Progress：进度条（按控件自身的归一化值 0~1 显示进度）。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::anim::AnimProp;
use crate::layout;
use crate::sizing::Sizing;
use crate::style::StyleSpec;
use crate::widget::{Base, Widget, WidgetProperty, WidgetRole};

/// 进度条：轨道用 bg_color（或默认灰），进度用 fg_color（或默认蓝），圆角胶囊。
pub struct Progress {
    base: Base,
    value: f32,
}

impl Progress {
    pub fn new() -> Self {
        let mut base = Base::new(WidgetRole::Plain);
        base.width = Sizing::Fill;
        base.height = Sizing::Fixed(8.0);
        Self { base, value: 0.0 }
    }
    /// 设置进度（0~1，自动夹取）。
    pub fn value(mut self, v: f32) -> Self {
        self.value = v.clamp(0.0, 1.0);
        self
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Progress {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        layout::size_from_content(&self.base, 120.0, 8.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        if content.size.width <= 0.0 || content.size.height <= 0.0 {
            return;
        }
        let track = style.bg_color.unwrap_or(Color::from_u8(60, 64, 74, 255));
        let fill = style.fg_color.unwrap_or(Color::from_u8(52, 120, 246, 255));
        let radius = Corners::all(content.size.height / 2.0);
        cv.fill_round_rect(content, radius, track);
        let v = self.value;
        let w = content.size.width * v;
        if w > 0.0 {
            let fill_rect = Rect::new(content.left(), content.top(), w, content.size.height);
            cv.fill_round_rect(fill_rect, radius, fill);
        }
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        if let WidgetProperty::Value(v) = property { self.value = v.clamp(0.0, 1.0); true } else { false }
    }
    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        (prop == AnimProp::Value).then_some(self.value)
    }
    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        if prop == AnimProp::Value { self.value = value.clamp(0.0, 1.0); true } else { false }
    }
}

common_builders!(Progress);
