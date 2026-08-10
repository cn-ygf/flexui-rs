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
        // 显示串 = 光标前文本 + IME 组合串 + 光标后文本；组合串加下划线以示未提交。
        let before: String = self.base.text.chars().take(self.base.cursor).collect();
        let after: String = self.base.text.chars().skip(self.base.cursor).collect();
        let display = format!("{before}{}{after}", self.base.marked);
        draw_aligned_text(cv, &display, content, &self.base.font, color, TextAlign::Left);

        let before_w = cv.measure_text(&before, &self.base.font).width;
        let caret_h = self.base.font.size;
        let cy = content.top() + (content.size.height - caret_h) / 2.0;
        // 组合串下划线。
        if !self.base.marked.is_empty() {
            let marked_w = cv.measure_text(&self.base.marked, &self.base.font).width;
            let uy = (cy + caret_h - 1.0).min(content.bottom() - 1.0);
            cv.fill_rect(
                Rect::new(content.left() + before_w, uy, marked_w.max(1.0), 1.0),
                color,
            );
        }
        // 仅在获得焦点且闪烁相位为亮时画光标；光标落在组合串之后；高度与字号一致、垂直居中。
        if self.base.focused && self.base.caret_on {
            let marked_w = cv.measure_text(&self.base.marked, &self.base.font).width;
            let cx = content.left() + before_w + marked_w + 1.0;
            cv.fill_rect(Rect::new(cx, cy.max(content.top()), 1.5, caret_h), color);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_node;
    use flexui_geometry::{Color, Corners, Point, Rect, Size};
    use flexui_gfx::Font;
    use std::cell::RefCell;

    /// 记录最后一次 draw_text 文本，用于验证 IME 组合串已内联绘制。
    struct RecCanvas {
        last_text: RefCell<String>,
    }
    impl Canvas for RecCanvas {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, t: &str, _o: Point, _f: &Font, _c: Color) {
            *self.last_text.borrow_mut() = t.to_string();
        }
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
    }

    #[test]
    fn ime_组合串内联绘制在光标处() {
        // 文本 "ac"，光标在中间（1），组合串 "b" → 显示 "abc"，text 不变。
        let mut e = Edit::new().text("ac");
        e.base_mut().cursor = 1;
        e.base_mut().marked = "b".to_string();
        e.base_mut().focused = true;
        let cv = RecCanvas { last_text: RefCell::new(String::new()) };
        let mut cv = cv;
        layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        let style = StyleSpec::default();
        e.paint_content(&mut cv, &style);
        assert_eq!(*cv.last_text.borrow(), "abc");
        assert_eq!(e.base().text, "ac", "组合串不改动已提交文本");
    }
}
