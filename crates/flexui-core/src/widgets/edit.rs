//! Edit：文本输入（选区、复制/剪切/粘贴、IME；单行/多行）。

use flexui_geometry::{Color, Point, Rect, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::event::{keys, Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::{PlaceholderStyleSet, StyleSpec};
use crate::widget::{Base, TextControl, Widget, WidgetRole};

/// 选区高亮色（半透明蓝）。
const SEL_COLOR: Color = Color::rgba(0.20, 0.45, 0.95, 0.35);
const PLACEHOLDER_COLOR: Color = Color::rgba(0.50, 0.50, 0.50, 1.0);

/// 单行的字符边界缓存：start 为该行首字符在整段文本中的索引，offsets 为行内每个
/// 字符边界的 x 偏移（累积前缀宽度，长度=行内字符数+1）。
struct LineCache {
    start: usize,
    offsets: Vec<f32>,
}

/// 文本输入控件（选区 + 剪贴板 + IME；单行/多行）。
pub struct Edit {
    base: Base,
    /// 按行的字符边界缓存，在 arrange 用真实测量算出，供 on_event 做 x/y→字符索引映射
    /// （否则 on_event 拿不到 Canvas）。单行时只有一项。
    lines: Vec<LineCache>,
    /// 行高（measure("Ag").height），arrange 时算出，供多行光标/命中定位。
    line_h: f32,
    /// 缓存是否因文本变更而过期（过期时映射回退到等宽估算）。
    cache_dirty: bool,
}

impl Edit {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Edit),
            lines: Vec::new(),
            line_h: 0.0,
            cache_dirty: true,
        }
    }
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.base.text = t.into();
        self.base.cursor = self.base.text.chars().count();
        self
    }
    /// 设置内容为空时显示的占位文本。
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.base.placeholder = text.into();
        self
    }
    /// 设置占位文本的分状态字体样式。
    pub fn placeholder_style(mut self, style: PlaceholderStyleSet) -> Self {
        self.base.placeholder_style = style;
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
        self.cache_dirty = true;
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
        self.cache_dirty = true;
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

    /// 行内本地 x → 最近字符边界列号（0..=行长）。缓存过期回退等宽估算。
    fn col_at(&self, li: usize, local_x: f32) -> usize {
        let est = |x: f32| ((x / (self.base.font.size * 0.6).max(1.0)).round().max(0.0)) as usize;
        if self.cache_dirty || li >= self.lines.len() {
            return est(local_x);
        }
        let offs = &self.lines[li].offsets;
        let mut best = 0;
        let mut best_d = f32::INFINITY;
        for (i, &x) in offs.iter().enumerate() {
            let d = (x - local_x).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// 当前行高（未布局时回退字号）。
    fn line_height(&self) -> f32 {
        if self.line_h > 0.0 {
            self.line_h
        } else {
            self.base.font.size
        }
    }

    /// 绝对坐标 → 字符索引（多行按 y 定位行，再按 x 定位列）。
    fn hit_index(&self, pos: Point) -> usize {
        let content = layout::content_rect(&self.base);
        let local_x = pos.x - content.left();
        if self.lines.is_empty() {
            let cw = (self.base.font.size * 0.6).max(1.0);
            return ((local_x / cw).round().max(0.0) as usize).min(self.char_count());
        }
        let li = if self.base.multiline {
            let rel = (pos.y - content.top()) / self.line_height();
            (rel.floor().max(0.0) as usize).min(self.lines.len() - 1)
        } else {
            0
        };
        let col = self.col_at(li, local_x).min(self.lines[li].offsets.len().saturating_sub(1));
        self.lines[li].start + col
    }

    /// 字符索引 → (行号, 列号)。缓存缺失时回退 (0, idx)。
    fn pos_of(&self, idx: usize) -> (usize, usize) {
        for (i, l) in self.lines.iter().enumerate() {
            let len = l.offsets.len().saturating_sub(1);
            if idx <= l.start + len {
                return (i, idx - l.start);
            }
        }
        match self.lines.last() {
            Some(l) => (self.lines.len() - 1, l.offsets.len().saturating_sub(1)),
            None => (0, idx),
        }
    }

    /// 上/下移一行（dir=-1/1），列尽量保持；extend 为 Shift 扩选。
    fn move_vertical(&mut self, dir: i32, extend: bool) {
        if self.lines.is_empty() {
            return;
        }
        let (line, col) = self.pos_of(self.base.cursor);
        let target = (line as i32 + dir).clamp(0, self.lines.len() as i32 - 1) as usize;
        let tlen = self.lines[target].offsets.len().saturating_sub(1);
        let nidx = self.lines[target].start + col.min(tlen);
        self.set_cursor(nidx, extend);
    }

    /// idx 处的词范围 [start,end)（按字母数字/其它二分类扩展，供双击选词）。
    fn word_range_at(&self, idx: usize) -> (usize, usize) {
        let chars: Vec<char> = self.base.text.chars().collect();
        let n = chars.len();
        if n == 0 {
            return (0, 0);
        }
        let i = idx.min(n);
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        // 探针取当前字符（在末尾则取前一个）决定分类。
        let probe = if i < n { chars[i] } else { chars[i - 1] };
        let cls = is_word(probe);
        let mut start = i.min(n - 1);
        // 若命中在词末尾边界，从 i-1 起。
        if i >= n {
            start = n - 1;
        }
        let mut end = start;
        while start > 0 && is_word(chars[start - 1]) == cls {
            start -= 1;
        }
        while end < n && is_word(chars[end]) == cls {
            end += 1;
        }
        (start, end)
    }

    /// 多行绘制：按 \n 分行逐行画文字，选区按行分段高亮，光标按行+列定位。
    fn paint_multiline(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::BLACK);
        let line_h = self.line_height();
        let caret_h = self.base.font.size;
        if self.shows_placeholder() {
            let style = self.base.placeholder_style.resolve(self.base.visual_state());
            let font = style.resolve_font(&self.base.font);
            let line_rect = Rect::new(content.left(), content.top(), content.size.width, line_h);
            draw_aligned_text(cv, &self.base.placeholder, line_rect, &font,
                style.fg_color.unwrap_or(PLACEHOLDER_COLOR), TextAlign::Left, false);
            if self.base.focused && self.base.caret_on {
                let y = content.top() + (line_h - caret_h) / 2.0;
                cv.fill_rect(Rect::new(content.left(), y, 1.5, caret_h), color);
            }
            return;
        }
        let sel = self.base.sel_range();
        let (cur_line, cur_col) = self.pos_of(self.base.cursor);

        let mut y = content.top();
        let mut base_idx = 0usize; // 行首字符索引
        for (i, line) in self.base.text.split('\n').enumerate() {
            let ll = line.chars().count();
            // 选区在本行的交集 [s,e)。
            if let Some((lo, hi)) = sel {
                let (ls, le) = (base_idx, base_idx + ll);
                let s = lo.max(ls);
                let e = hi.min(le);
                if s < e {
                    let pre: String = line.chars().take(s - ls).collect();
                    let mid: String = line.chars().skip(s - ls).take(e - s).collect();
                    let x0 = content.left() + cv.measure_text_advance(&pre, &self.base.font);
                    let w = cv.measure_text_advance(&mid, &self.base.font);
                    cv.fill_rect(Rect::new(x0, y, w.max(1.0), line_h), SEL_COLOR);
                }
            }
            // 文字。
            let line_rect = Rect::new(content.left(), y, content.size.width, line_h);
            Self::draw_input_text(cv, line, line_rect, &self.base.font, color);
            // 光标（仅当前行）。
            if self.base.focused && self.base.caret_on && i == cur_line {
                let pre: String = line.chars().take(cur_col).collect();
                let cx = content.left() + cv.measure_text_advance(&pre, &self.base.font) + 1.0;
                let cyc = y + (line_h - caret_h) / 2.0;
                cv.fill_rect(Rect::new(cx, cyc.max(y), 1.5, caret_h), color);
            }
            y += line_h;
            base_idx += ll + 1;
        }
    }

    fn shows_placeholder(&self) -> bool {
        self.base.text.is_empty() && self.base.marked.is_empty() && !self.base.placeholder.is_empty()
    }

    /// 输入文本使用与字符前进宽度相同的排版方式，确保光标紧贴字符边界。
    fn draw_input_text(
        cv: &mut dyn Canvas,
        text: &str,
        rect: Rect,
        font: &flexui_gfx::Font,
        color: Color,
    ) {
        if text.is_empty() {
            return;
        }
        let height = cv.measure_text(text, font).height;
        let y = rect.top() + (rect.size.height - height) / 2.0;
        cv.draw_text_advance(text, Point::new(rect.left(), y.max(rect.top())), font, color);
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
        if self.base.multiline {
            let rows = self.base.text.split('\n').count().max(1) as f32;
            layout::size_from_content(&self.base, 120.0, rows * s.height + 8.0)
        } else {
            layout::size_from_content(&self.base, 120.0, s.height + 8.0)
        }
    }
    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        layout::arrange_stack(&mut self.base, content, cv);
        // 按行缓存字符边界 x 偏移（累积前缀 → CJK/kerning 正确），供 x/y→索引映射。
        let text = self.base.text.clone();
        let mut lines = Vec::new();
        let mut start = 0usize;
        for line in text.split('\n') {
            let n = line.chars().count();
            let mut offs = Vec::with_capacity(n + 1);
            for i in 0..=n {
                let pre: String = line.chars().take(i).collect();
                offs.push(cv.measure_text_advance(&pre, &self.base.font));
            }
            lines.push(LineCache { start, offsets: offs });
            start += n + 1; // +1 跳过换行符
        }
        self.lines = lines;
        self.line_h = cv.measure_text("Ag", &self.base.font).height;
        self.cache_dirty = false;
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        if self.base.multiline {
            self.paint_multiline(cv, style);
            return;
        }
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::BLACK);
        let caret_h = self.base.font.size;
        let cy = content.top() + (content.size.height - caret_h) / 2.0;
        if self.shows_placeholder() {
            let style = self.base.placeholder_style.resolve(self.base.visual_state());
            let font = style.resolve_font(&self.base.font);
            draw_aligned_text(cv, &self.base.placeholder, content, &font,
                style.fg_color.unwrap_or(PLACEHOLDER_COLOR), TextAlign::Left, false);
            if self.base.focused && self.base.caret_on {
                cv.fill_rect(Rect::new(content.left(), cy.max(content.top()), 1.5, caret_h), color);
            }
            return;
        }
        // 选区高亮（组合中不画）：先在文字底下铺一条半透明矩形。
        if self.base.marked.is_empty() {
            if let Some((lo, hi)) = self.base.sel_range() {
                let pre: String = self.base.text.chars().take(lo).collect();
                let sel: String = self.base.text.chars().skip(lo).take(hi - lo).collect();
                let x0 = content.left() + cv.measure_text_advance(&pre, &self.base.font);
                let w = cv.measure_text_advance(&sel, &self.base.font);
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
        Self::draw_input_text(cv, &display, content, &self.base.font, color);

        let before_w = cv.measure_text_advance(&before, &self.base.font);
        // 组合串下划线。
        if !self.base.marked.is_empty() {
            let marked_w = cv.measure_text_advance(&self.base.marked, &self.base.font);
            let uy = (cy + caret_h - 1.0).min(content.bottom() - 1.0);
            cv.fill_rect(
                Rect::new(content.left() + before_w, uy, marked_w.max(1.0), 1.0),
                color,
            );
        }
        // 仅在获得焦点且闪烁相位为亮时画光标；光标落在组合串之后；高度与字号一致、垂直居中。
        if self.base.focused && self.base.caret_on {
            let marked_w = cv.measure_text_advance(&self.base.marked, &self.base.font);
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
            // 鼠标按下：光标定位到点击处，并把锚点也置于此（拖动即从这里起选）。
            Event::MouseDown { pos, button: MouseButton::Left } => {
                let idx = self.hit_index(*pos);
                self.base.cursor = idx;
                self.base.sel_anchor = Some(idx);
                EventFlow::Consumed
            }
            // 鼠标拖动（分发器仅在按住本控件时转发）：延伸选区到当前位置。
            Event::MouseMove { pos } => {
                self.base.cursor = self.hit_index(*pos);
                EventFlow::Consumed
            }
            // 双击：选中光标处的词。
            Event::DoubleClick { pos } => {
                let idx = self.hit_index(*pos);
                let (start, end) = self.word_range_at(idx);
                self.base.sel_anchor = Some(start);
                self.base.cursor = end;
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
                    // 多行：到本行行首；单行：到文本开头。
                    let target = if self.base.multiline && !self.lines.is_empty() {
                        let (line, _) = self.pos_of(self.base.cursor);
                        self.lines[line].start
                    } else {
                        0
                    };
                    self.set_cursor(target, mods.shift);
                    EventFlow::Consumed
                }
                keys::END => {
                    let target = if self.base.multiline && !self.lines.is_empty() {
                        let (line, _) = self.pos_of(self.base.cursor);
                        let len = self.lines[line].offsets.len().saturating_sub(1);
                        self.lines[line].start + len
                    } else {
                        self.char_count()
                    };
                    self.set_cursor(target, mods.shift);
                    EventFlow::Consumed
                }
                keys::ENTER if self.base.multiline => {
                    self.insert_str("\n");
                    EventFlow::Consumed
                }
                keys::UP if self.base.multiline => {
                    self.move_vertical(-1, mods.shift);
                    EventFlow::Consumed
                }
                keys::DOWN if self.base.multiline => {
                    self.move_vertical(1, mods.shift);
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
        last_font: RefCell<Option<Font>>,
        last_color: RefCell<Option<Color>>,
    }
    impl Canvas for RecCanvas {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, t: &str, _o: Point, f: &Font, c: Color) {
            *self.last_text.borrow_mut() = t.to_string();
            *self.last_font.borrow_mut() = Some(f.clone());
            *self.last_color.borrow_mut() = Some(c);
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
        let cv = RecCanvas { last_text: RefCell::new(String::new()), last_font: RefCell::new(None), last_color: RefCell::new(None) };
        let mut cv = cv;
        layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        let style = StyleSpec::default();
        e.paint_content(&mut cv, &style);
        assert_eq!(*cv.last_text.borrow(), "abc");
        assert_eq!(e.base().text, "ac", "组合串不改动已提交文本");
    }

    fn rec_canvas() -> RecCanvas {
        RecCanvas { last_text: RefCell::new(String::new()), last_font: RefCell::new(None), last_color: RefCell::new(None) }
    }

    use crate::event::{keys, Event, Mods};

    fn kd(key: u32, shift: bool) -> Event {
        Event::KeyDown { key, mods: Mods { shift, ..Default::default() } }
    }

    #[test]
    fn placeholder_only_draws_for_empty_text_and_returns_after_delete() {
        let mut edit = Edit::new().placeholder("请输入");
        let mut cv = rec_canvas();
        layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        edit.paint_content(&mut cv, &StyleSpec::default());
        assert_eq!(*cv.last_text.borrow(), "请输入");
        assert!(edit.base().text.is_empty(), "占位文本不能成为输入内容");
        edit.on_event(&Event::Char { ch: 'A' });
        edit.paint_content(&mut cv, &StyleSpec::default());
        assert_eq!(*cv.last_text.borrow(), "A");
        edit.on_event(&kd(keys::BACKSPACE, false));
        edit.paint_content(&mut cv, &StyleSpec::default());
        assert_eq!(*cv.last_text.borrow(), "请输入");
    }

    #[test]
    fn placeholder_draws_complete_font_for_current_state() {
        use crate::style::{BaseState, PlaceholderStyleSpec, VisualState};
        let mut styles = PlaceholderStyleSet::new().with_normal(PlaceholderStyleSpec {
            font_family: Some("Microsoft YaHei".to_string()), font_size: Some(13.0),
            fg_color: Some(Color::WHITE), bold: Some(true), italic: Some(false), underline: Some(true),
        });
        styles.set(VisualState::new(BaseState::Hot, false), PlaceholderStyleSpec {
            font_size: Some(15.0), fg_color: Some(Color::BLACK), italic: Some(true), ..Default::default()
        });
        let mut edit = Edit::new().placeholder("搜索").placeholder_style(styles);
        edit.base_mut().hover = true;
        let mut cv = rec_canvas();
        layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        edit.paint_content(&mut cv, &StyleSpec::default());
        let font = cv.last_font.borrow().clone().unwrap();
        assert_eq!(font.family.as_deref(), Some("Microsoft YaHei"));
        assert_eq!(font.size, 15.0);
        assert!(font.bold && font.italic && font.underline);
        assert_eq!(*cv.last_color.borrow(), Some(Color::BLACK));
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
    fn 多行_enter插换行与上下移行() {
        let cv = FakeCanvas;
        let mut e = Edit::new().multiline(true).text("ab");
        // 末尾回车 → "ab\n"，光标在第2行行首。
        e.on_event(&kd(keys::ENTER, false));
        assert_eq!(e.base().text, "ab\n");
        e.on_event(&Event::Char { ch: 'c' });
        e.on_event(&Event::Char { ch: 'd' }); // "ab\ncd"
        assert_eq!(e.base().text, "ab\ncd");
        // 布局以建立行缓存。
        layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 80.0), &cv);
        // 光标此时在末尾(第2行 col2)。上移到第1行同列(col2 → "ab"末尾, 索引2)。
        e.on_event(&kd(keys::UP, false));
        assert_eq!(e.base().cursor, 2);
        // 下移回第2行同列(col2 → 索引5)。
        e.on_event(&kd(keys::DOWN, false));
        assert_eq!(e.base().cursor, 5);
    }

    #[test]
    fn 多行_measure高度随行数增长() {
        let cv = FakeCanvas;
        let mut one = Edit::new().multiline(true).text("a");
        let mut three = Edit::new().multiline(true).text("a\nb\nc");
        let h1 = one.measure(Size::new(200.0, 200.0), &cv).height;
        let h3 = three.measure(Size::new(200.0, 200.0), &cv).height;
        assert!(h3 > h1, "三行应比一行高: {h3} vs {h1}");
    }

    struct FakeCanvas;
    impl Canvas for FakeCanvas {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, _t: &str, _o: Point, _f: &Font, _c: Color) {}
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
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
