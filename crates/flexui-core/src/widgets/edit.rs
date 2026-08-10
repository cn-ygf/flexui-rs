//! Edit：单行文本输入（基础版：Char 追加、Backspace 删除；focus 时显示光标）。

use flexui_geometry::{Color, Rect, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::event::{keys, Event, EventFlow};
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
        self.base.cursor = self.base.text.chars().count();
        self
    }

    fn char_count(&self) -> usize {
        self.base.text.chars().count()
    }
    /// 在光标处插入字符。
    fn insert(&mut self, ch: char) {
        let mut chars: Vec<char> = self.base.text.chars().collect();
        let idx = self.base.cursor.min(chars.len());
        chars.insert(idx, ch);
        self.base.text = chars.into_iter().collect();
        self.base.cursor = idx + 1;
    }
    /// 删除光标前一个字符（退格）。
    fn backspace(&mut self) {
        if self.base.cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.base.text.chars().collect();
        let idx = self.base.cursor - 1;
        if idx < chars.len() {
            chars.remove(idx);
            self.base.text = chars.into_iter().collect();
        }
        self.base.cursor = idx;
    }
    /// 删除光标处字符（Delete）。
    fn delete_forward(&mut self) {
        let mut chars: Vec<char> = self.base.text.chars().collect();
        let idx = self.base.cursor;
        if idx < chars.len() {
            chars.remove(idx);
            self.base.text = chars.into_iter().collect();
        }
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
        // focus 时在光标位置画竖线（度量光标前子串宽度）。
        if self.base.focused {
            let before: String = self.base.text.chars().take(self.base.cursor).collect();
            let tw = cv.measure_text(&before, &self.base.font).width;
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
                self.insert(*ch);
                EventFlow::Consumed
            }
            Event::KeyDown { key } => match *key {
                keys::BACKSPACE => {
                    self.backspace();
                    EventFlow::Consumed
                }
                keys::DELETE => {
                    self.delete_forward();
                    EventFlow::Consumed
                }
                keys::LEFT => {
                    self.base.cursor = self.base.cursor.saturating_sub(1);
                    EventFlow::Consumed
                }
                keys::RIGHT => {
                    self.base.cursor = (self.base.cursor + 1).min(self.char_count());
                    EventFlow::Consumed
                }
                keys::HOME => {
                    self.base.cursor = 0;
                    EventFlow::Consumed
                }
                keys::END => {
                    self.base.cursor = self.char_count();
                    EventFlow::Consumed
                }
                _ => EventFlow::Ignored,
            },
            _ => EventFlow::Ignored,
        }
    }
}

common_builders!(Edit);

impl TextControl for Edit {}
