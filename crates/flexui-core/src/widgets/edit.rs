//! Edit：文本输入（选区、复制/剪切/粘贴、IME；单行/多行）。

use flexui_geometry::{Color, Rect, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::event::{keys, Event, EventFlow};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::widget::{Base, TextControl, Widget, WidgetRole};

/// 选区高亮色（半透明蓝）。
const SEL_COLOR: Color = Color::rgba(0.20, 0.45, 0.95, 0.35);

/// 文本输入控件（选区 + 剪贴板 + IME；单行/多行）。
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
    /// 设为多行文本（Enter 换行）。
    pub fn multiline(mut self, on: bool) -> Self {
        self.base.multiline = on;
        self
    }

    fn char_count(&self) -> usize {
        self.base.text.chars().count()
    }

    /// 移动光标到 idx；extend=true 时按住 Shift 扩展选区（保留/建立锚点），否则清锚点。
    fn set_cursor(&mut self, idx: usize, extend: bool) {
        if extend {
            if self.base.sel_anchor.is_none() {
                self.base.sel_anchor = Some(self.base.cursor);
            }
        } else {
            self.base.sel_anchor = None;
        }
        self.base.cursor = idx.min(self.char_count());
    }

    /// 删除 [lo,hi) 字符；光标落到 lo，清锚点。
    fn delete_range(&mut self, lo: usize, hi: usize) {
        let mut chars: Vec<char> = self.base.text.chars().collect();
        let hi = hi.min(chars.len());
        let lo = lo.min(hi);
        if lo >= hi {
            self.base.sel_anchor = None;
            return;
        }
        chars.drain(lo..hi);
        self.base.text = chars.into_iter().collect();
        self.base.cursor = lo;
        self.base.sel_anchor = None;
    }

    /// 在光标处插入字符串；若有选区则先替换掉选区。
    fn insert_str(&mut self, s: &str) {
        if let Some((lo, hi)) = self.base.sel_range() {
            self.delete_range(lo, hi);
        }
        let mut chars: Vec<char> = self.base.text.chars().collect();
        let idx = self.base.cursor.min(chars.len());
        let ins: Vec<char> = s.chars().collect();
        let n = ins.len();
        for (k, c) in ins.into_iter().enumerate() {
            chars.insert(idx + k, c);
        }
        self.base.text = chars.into_iter().collect();
        self.base.cursor = idx + n;
        self.base.sel_anchor = None;
    }

    /// 在光标处插入单个字符（选区替换语义同 insert_str）。
    fn insert(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        self.insert_str(ch.encode_utf8(&mut buf));
    }

    /// 退格：有选区删选区，否则删光标前一个字符。
    fn backspace(&mut self) {
        if let Some((lo, hi)) = self.base.sel_range() {
            self.delete_range(lo, hi);
            return;
        }
        if self.base.cursor == 0 {
            return;
        }
        let idx = self.base.cursor - 1;
        self.delete_range(idx, idx + 1);
    }

    /// Delete：有选区删选区，否则删光标处字符。
    fn delete_forward(&mut self) {
        if let Some((lo, hi)) = self.base.sel_range() {
            self.delete_range(lo, hi);
            return;
        }
        let idx = self.base.cursor;
        self.delete_range(idx, idx + 1);
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
        let caret_h = self.base.font.size;
        let cy = content.top() + (content.size.height - caret_h) / 2.0;
        // 选区高亮（组合中不画）：先在文字底下铺一条半透明矩形。
        if self.base.marked.is_empty() {
            if let Some((lo, hi)) = self.base.sel_range() {
                let pre: String = self.base.text.chars().take(lo).collect();
                let sel: String = self.base.text.chars().skip(lo).take(hi - lo).collect();
                let x0 = content.left() + cv.measure_text(&pre, &self.base.font).width;
                let w = cv.measure_text(&sel, &self.base.font).width;
                cv.fill_rect(
                    Rect::new(x0, cy.max(content.top()), w.max(1.0), caret_h),
                    SEL_COLOR,
                );
            }
        }
        // 显示串 = 光标前文本 + IME 组合串 + 光标后文本；组合串加下划线以示未提交。
        let before: String = self.base.text.chars().take(self.base.cursor).collect();
        let after: String = self.base.text.chars().skip(self.base.cursor).collect();
        let display = format!("{before}{}{after}", self.base.marked);
        draw_aligned_text(cv, &display, content, &self.base.font, color, TextAlign::Left);

        let before_w = cv.measure_text(&before, &self.base.font).width;
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
            Event::KeyDown { key, mods } => match *key {
                keys::BACKSPACE => {
                    self.backspace();
                    EventFlow::Consumed
                }
                keys::DELETE => {
                    self.delete_forward();
                    EventFlow::Consumed
                }
                keys::LEFT => {
                    // 有选区且不按 Shift：收起到选区左端；否则左移（Shift 扩展）。
                    if !mods.shift {
                        if let Some((lo, _)) = self.base.sel_range() {
                            self.base.cursor = lo;
                            self.base.sel_anchor = None;
                        } else {
                            self.set_cursor(self.base.cursor.saturating_sub(1), false);
                        }
                    } else {
                        self.set_cursor(self.base.cursor.saturating_sub(1), true);
                    }
                    EventFlow::Consumed
                }
                keys::RIGHT => {
                    if !mods.shift {
                        if let Some((_, hi)) = self.base.sel_range() {
                            self.base.cursor = hi;
                            self.base.sel_anchor = None;
                        } else {
                            self.set_cursor(self.base.cursor + 1, false);
                        }
                    } else {
                        self.set_cursor(self.base.cursor + 1, true);
                    }
                    EventFlow::Consumed
                }
                keys::HOME => {
                    self.set_cursor(0, mods.shift);
                    EventFlow::Consumed
                }
                keys::END => {
                    let n = self.char_count();
                    self.set_cursor(n, mods.shift);
                    EventFlow::Consumed
                }
                _ => EventFlow::Ignored,
            },
            _ => EventFlow::Ignored,
        }
    }

    fn selected_text(&self) -> Option<String> {
        let (lo, hi) = self.base.sel_range()?;
        Some(self.base.text.chars().skip(lo).take(hi - lo).collect())
    }
    fn replace_selection(&mut self, s: &str) -> bool {
        let had = self.base.sel_range().is_some();
        if !had && s.is_empty() {
            return false;
        }
        self.insert_str(s);
        true
    }
    fn delete_selection(&mut self) -> bool {
        if let Some((lo, hi)) = self.base.sel_range() {
            self.delete_range(lo, hi);
            true
        } else {
            false
        }
    }
    fn select_all(&mut self) {
        self.base.sel_anchor = Some(0);
        self.base.cursor = self.char_count();
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

    use crate::event::{keys, Event, Mods};

    fn kd(key: u32, shift: bool) -> Event {
        Event::KeyDown { key, mods: Mods { shift, ..Default::default() } }
    }

    #[test]
    fn 选区_shift方向扩展与收起() {
        let mut e = Edit::new().text("hello"); // cursor=5
        // Shift+Left ×2 选中 "lo"
        e.on_event(&kd(keys::LEFT, true));
        e.on_event(&kd(keys::LEFT, true));
        assert_eq!(e.base().sel_range(), Some((3, 5)));
        assert_eq!(e.selected_text().as_deref(), Some("lo"));
        // 平移 Left：收起到左端 3，无选区
        e.on_event(&kd(keys::LEFT, false));
        assert_eq!(e.base().cursor, 3);
        assert_eq!(e.base().sel_range(), None);
    }

    #[test]
    fn 选区_typing替换选区() {
        let mut e = Edit::new().text("hello");
        e.on_event(&kd(keys::HOME, false)); // cursor=0
        e.on_event(&kd(keys::RIGHT, true)); // 选中 "h"
        e.on_event(&kd(keys::RIGHT, true)); // 选中 "he"
        assert_eq!(e.selected_text().as_deref(), Some("he"));
        e.on_event(&Event::Char { ch: 'X' }); // 替换为 X
        assert_eq!(e.base().text, "Xllo");
        assert_eq!(e.base().cursor, 1);
        assert_eq!(e.base().sel_range(), None);
    }

    #[test]
    fn 选区_全选与钩子() {
        let mut e = Edit::new().text("abcd");
        e.select_all();
        assert_eq!(e.selected_text().as_deref(), Some("abcd"));
        // 粘贴替换
        assert!(e.replace_selection("Z"));
        assert_eq!(e.base().text, "Z");
        // 无选区+空串 → 不改变
        assert!(!e.replace_selection(""));
        // 删选区
        e.select_all();
        assert!(e.delete_selection());
        assert_eq!(e.base().text, "");
        assert!(!e.delete_selection());
    }

    #[test]
    fn 折叠锚点不算选区() {
        let mut e = Edit::new().text("ab");
        e.on_event(&kd(keys::LEFT, true)); // anchor=2 cursor=1
        e.on_event(&kd(keys::RIGHT, true)); // cursor 回到 2 == anchor
        assert_eq!(e.base().sel_range(), None, "锚点与光标重合视为无选区");
        assert_eq!(e.selected_text(), None);
    }
}
