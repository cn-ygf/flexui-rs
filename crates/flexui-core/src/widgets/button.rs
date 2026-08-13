//! Button：按钮控件（完整 4×2 状态，点击触发回调/通知）。

use flexui_geometry::{Color, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Clickable, TextControl, Widget, WidgetRole};

/// 按钮：完整 4×2 状态，点击触发回调。
pub struct Button {
    base: Base,
}

impl Button {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new_kind(WidgetRole::Button, WidgetKind::Button);
        base.text = text.into();
        Self { base }
    }
}

impl Widget for Button {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        // 文字 + 默认左右各 12 的内边距（若已设 padding 则叠加）。
        layout::size_from_content(&self.base, s.width + 24.0, s.height + 12.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let color = style.fg_color.unwrap_or(Color::WHITE);
        let align = style.text_align.unwrap_or(TextAlign::Center);
        draw_aligned_text(
            cv,
            &self.base.text,
            layout::content_rect(&self.base),
            &self.base.font,
            color,
            align,
            true,
        );
    }
}

common_builders!(Button);

impl TextControl for Button {}
impl Clickable for Button {}
