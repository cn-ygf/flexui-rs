//! Edit：单行文本输入（基础版：Char 追加、Backspace 删除；focus 时显示光标）。

use flexui_geometry::{Color, Rect, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::event::{Event, EventFlow};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::widget::{Base, TextControl, Widget, WidgetRole};

/// 单行文本输入（基础版：接收 Char 追加、Backspace 删除；focus 时显示光标）。
pub struct Edit {
    base: Base,
}

impl Edit {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Edit),
        }
    }
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.base.text = t.into();
        self
    }
}

impl Default for Edit {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Edit {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text("Ag", &self.base.font);
        layout::size_from_content(&self.base, 120.0, s.height + 8.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::BLACK);
        draw_aligned_text(cv, &self.base.text, content, &self.base.font, color, TextAlign::Left);
        // focus 时在文字末尾画一根光标。
        if self.base.focused {
            let tw = cv.measure_text(&self.base.text, &self.base.font).width;
            let cx = content.left() + tw + 1.0;
            cv.fill_rect(
                Rect::new(cx, content.top() + 2.0, 1.5, content.size.height - 4.0),
                color,
            );
        }
    }
    fn on_event(&mut self, ev: &Event) -> EventFlow {
        match ev {
            Event::Char { ch } if !ch.is_control() => {
                self.base.text.push(*ch);
                EventFlow::Consumed
            }
            // 退格键（macOS/Win 退格常见键码 8）。
            Event::KeyDown { key } if *key == 8 => {
                self.base.text.pop();
                EventFlow::Consumed
            }
            _ => EventFlow::Ignored,
        }
    }
}

common_builders!(Edit);

impl TextControl for Edit {}
