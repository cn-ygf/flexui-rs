//! Slider：滑块（拖动改变控件自身的归一化值 0~1）。
//!
//! 交互事件（按下/拖动）由分发器转发到本控件（见 dispatch 的指针转发）。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::Canvas;

use crate::anim::AnimProp;
use crate::common_builders;
use crate::event::{Event, EventFlow, MouseButton};
use crate::layout;
use crate::sizing::Sizing;
use crate::style::StyleSpec;
use crate::widget::{Base, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole};

/// 滑块：轨道 + 已填充段 + 圆形拖柄。value 归一化 0~1。
pub struct Slider {
    base: Base,
    value: f32,
}

impl Slider {
    pub fn new() -> Self {
        let mut base = Base::new(WidgetRole::Slider);
        base.width = Sizing::Fill;
        base.height = Sizing::Fixed(24.0);
        Self { base, value: 0.0 }
    }
    /// 设置初值（0~1，自动夹取）。
    pub fn value(mut self, v: f32) -> Self {
        self.value = v.clamp(0.0, 1.0);
        self
    }

    /// 由绝对坐标 x 计算并设置 value。
    fn set_value_from_x(&mut self, x: f32) {
        let content = layout::content_rect(&self.base);
        if content.size.width <= 0.0 {
            return;
        }
        let frac = ((x - content.left()) / content.size.width).clamp(0.0, 1.0);
        self.value = frac;
    }
}

impl Default for Slider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Slider {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        layout::size_from_content(&self.base, 160.0, 24.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        if content.size.width <= 0.0 || content.size.height <= 0.0 {
            return;
        }
        let track_col = style.bg_color.unwrap_or(Color::from_u8(60, 64, 74, 255));
        let fill_col = style.fg_color.unwrap_or(Color::from_u8(52, 120, 246, 255));
        let knob_col = Color::from_u8(240, 244, 250, 255);

        let track_h = 4.0f32.min(content.size.height);
        let ty = content.top() + (content.size.height - track_h) / 2.0;
        let track = Rect::new(content.left(), ty, content.size.width, track_h);
        cv.fill_round_rect(track, Corners::all(track_h / 2.0), track_col);

        let v = self.value;
        let fill_w = content.size.width * v;
        if fill_w > 0.0 {
            cv.fill_round_rect(
                Rect::new(content.left(), ty, fill_w, track_h),
                Corners::all(track_h / 2.0),
                fill_col,
            );
        }
        // 拖柄：直径取控件高度，圆心在 value 处。
        let d = content.size.height;
        let cx = content.left() + fill_w;
        let knob = Rect::new(cx - d / 2.0, content.top(), d, d);
        cv.fill_round_rect(knob, Corners::all(d / 2.0), knob_col);
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        if let WidgetProperty::Value(v) = property {
            self.value = v.clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }
    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        (key == WidgetPropertyKey::Value).then_some(WidgetProperty::Value(self.value))
    }
    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        (prop == AnimProp::Value).then_some(self.value)
    }
    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        if prop == AnimProp::Value {
            self.value = value.clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }
    fn on_event(&mut self, ev: &Event) -> EventFlow {
        match ev {
            // 按下定位到点击处；拖动（分发器仅在按住本控件时转发 MouseMove）持续更新。
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
            } => {
                self.set_value_from_x(pos.x);
                EventFlow::Consumed
            }
            Event::MouseMove { pos } => {
                self.set_value_from_x(pos.x);
                EventFlow::Consumed
            }
            _ => EventFlow::Ignored,
        }
    }
}

common_builders!(Slider);

impl Slider {
    /// 当前值（0~1）。
    pub fn current(&self) -> f32 {
        self.value
    }
}
