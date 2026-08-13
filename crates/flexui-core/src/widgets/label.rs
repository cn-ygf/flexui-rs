//! Label：文本标签控件（不可交互）。

use flexui_geometry::{Color, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, TextControl, Widget, WidgetRole};

/// 文本标签（不可交互）。
pub struct Label {
    base: Base,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new_kind(WidgetRole::Plain, WidgetKind::Label);
        base.text = text.into();
        Self { base }
    }
}

impl Widget for Label {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        layout::size_from_content(&self.base, s.width, s.height)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let color = style.fg_color.unwrap_or(Color::BLACK);
        let align = style.text_align.unwrap_or(TextAlign::Left);
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

common_builders!(Label);

impl TextControl for Label {}
