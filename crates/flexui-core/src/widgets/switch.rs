//! Switch：独立的开关控件，点击切换 selected。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout;
use crate::sizing::Sizing;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Clickable, Widget, WidgetRole};

pub struct Switch {
    base: Base,
}

impl Switch {
    pub fn new() -> Self {
        let mut base = Base::new_kind(WidgetRole::CheckBox, WidgetKind::Switch);
        base.width = Sizing::Fixed(44.0);
        base.height = Sizing::Fixed(24.0);
        Self { base }
    }

    pub fn checked(mut self, value: bool) -> Self {
        self.base.selected = value;
        self
    }
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Switch {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        layout::size_from_content(&self.base, 44.0, 24.0)
    }

    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        if style.bg_image.is_some() || style.fg_image.is_some() {
            return;
        }
        let content = layout::content_rect(&self.base);
        let track = Rect::new(
            content.left(),
            content.top(),
            content.size.width,
            content.size.height,
        );
        let track_color = style
            .track_color
            .unwrap_or_else(|| Color::from_u8(140, 150, 170, 255));
        cv.fill_round_rect(track, Corners::all(content.size.height / 2.0), track_color);

        let diameter = (content.size.height - 8.0).max(8.0);
        let x = if self.base.selected {
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
                .thumb_color
                .unwrap_or_else(|| Color::from_u8(255, 255, 255, 255)),
        );
    }
}

common_builders!(Switch);

impl Clickable for Switch {}
