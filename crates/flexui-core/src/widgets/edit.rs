//! Edit：文本输入（选区、复制/剪切/粘贴、IME；单行/多行）。

use std::cell::Cell;

use flexui_geometry::{Color, Point, Rect, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::event::{keys, Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::{PlaceholderStyleSet, StyleSpec};
use crate::widget::{
    Base, TextControl, TextInputState, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole,
};

/// 选区高亮色（半透明蓝）。
const SEL_COLOR: Color = Color::rgba(0.20, 0.45, 0.95, 0.35);
const PLACEHOLDER_COLOR: Color = Color::rgba(0.50, 0.50, 0.50, 1.0);

/// 单行的字符边界缓存：start 为该行首字符在整段文本中的索引，offsets 为行内每个
/// 字符边界的 x 偏移（累积前缀宽度，长度=行内字符数+1）。
struct LineCache {
    start: usize,
    offsets: Vec<f32>,
}

/// Edit 的持久配置，不污染所有控件共享的 Base。
pub struct EditConfig {
    pub placeholder: String,
    pub placeholder_style: PlaceholderStyleSet,
    pub multiline: bool,
    pub read_only: bool,
    pub number_only: bool,
    pub password: bool,
    pub password_char: char,
    pub max_chars: Option<usize>,
    pub auto_select_all: bool,
}

impl Default for EditConfig {
    fn default() -> Self {
        Self {
            placeholder: String::new(),
            placeholder_style: PlaceholderStyleSet::new(),
            multiline: false,
            read_only: false,
            number_only: false,
            password: false,
            password_char: '\u{2022}',
            max_chars: None,
            auto_select_all: false,
        }
    }
}

#[derive(Default)]
struct EditState {
    cursor: usize,
    sel_anchor: Option<usize>,
    marked: String,
}

/// 文本输入控件（选区 + 剪贴板 + IME；单行/多行）。
pub struct Edit {
    base: Base,
    config: EditConfig,
    state: EditState,
    /// 按行的字符边界缓存，在 arrange 用真实测量算出，供 on_event 做 x/y→字符索引映射
    /// （否则 on_event 拿不到 Canvas）。单行时只有一项。
    lines: Vec<LineCache>,
    /// 行高（measure("Ag").height），arrange 时算出，供多行光标/命中定位。
    line_h: f32,
    /// 缓存是否因文本变更而过期（过期时映射回退到等宽估算）。
    cache_dirty: bool,
    /// 单行内容向左卷起的逻辑像素。
    scroll_x: Cell<f32>,
    /// 最近一次排版/绘制得到的真实插入点矩形。
    caret_rect: Cell<Option<Rect>>,
}

impl Edit {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Edit),
            config: EditConfig::default(),
            state: EditState::default(),
            lines: Vec::new(),
            line_h: 0.0,
            cache_dirty: true,
            scroll_x: Cell::new(0.0),
            caret_rect: Cell::new(None),
        }
    }
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.set_text_value(t.into());
        self
    }
    /// 设置内容为空时显示的占位文本。
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.config.placeholder = text.into();
        self
    }
    /// 设置占位文本的分状态字体样式。
    pub fn placeholder_style(mut self, style: PlaceholderStyleSet) -> Self {
        self.config.placeholder_style = style;
        self
    }
    /// 设为多行文本（Enter 换行）。
    pub fn multiline(mut self, on: bool) -> Self {
        self.config.multiline = on;
        self
    }
    pub fn read_only(mut self, on: bool) -> Self {
        self.config.read_only = on;
        self
    }
    pub fn number_only(mut self, on: bool) -> Self {
        self.config.number_only = on;
        self.normalize_text();
        self
    }
    pub fn password(mut self, on: bool) -> Self {
        self.config.password = on;
        self.cache_dirty = true;
        self
    }
    pub fn password_char(mut self, ch: char) -> Self {
        self.config.password_char = ch;
        self.cache_dirty = true;
        self
    }
    pub fn max_chars(mut self, max: usize) -> Self {
        self.config.max_chars = Some(max);
        self.normalize_text();
        self
    }
    pub fn auto_select_all(mut self, on: bool) -> Self {
        self.config.auto_select_all = on;
        self
    }

    pub fn config(&self) -> &EditConfig {
        &self.config
    }
    pub fn cursor(&self) -> usize {
        self.state.cursor
    }
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.sel_range()
    }

    fn sel_range(&self) -> Option<(usize, usize)> {
        let anchor = self.state.sel_anchor?;
        if anchor == self.state.cursor {
            None
        } else {
            Some((anchor.min(self.state.cursor), anchor.max(self.state.cursor)))
        }
    }

    fn char_count(&self) -> usize {
        self.base.text.chars().count()
    }

    fn normalize_text(&mut self) {
        let mut text: String = self
            .base
            .text
            .chars()
            .filter(|ch| !self.config.number_only || ch.is_ascii_digit())
            .collect();
        if let Some(max) = self.config.max_chars {
            text = text.chars().take(max).collect();
        }
        self.base.text = text;
        self.state.cursor = self.state.cursor.min(self.char_count());
        self.state.sel_anchor = self.state.sel_anchor.map(|a| a.min(self.char_count()));
        self.cache_dirty = true;
    }

    fn accepted_input(&self, s: &str) -> String {
        let selected = self.sel_range().map_or(0, |(lo, hi)| hi - lo);
        let available = self
            .config
            .max_chars
            .map(|max| max.saturating_sub(self.char_count().saturating_sub(selected)))
            .unwrap_or(usize::MAX);
        s.chars()
            .filter(|ch| {
                (!self.config.number_only || ch.is_ascii_digit())
                    && (self.config.multiline || (*ch != '\n' && *ch != '\r'))
            })
            .take(available)
            .collect()
    }

    fn display_slice(&self, s: &str) -> String {
        if self.config.password {
            std::iter::repeat_n(self.config.password_char, s.chars().count()).collect()
        } else {
            s.to_string()
        }
    }

    /// 移动光标到 idx；extend=true 时按住 Shift 扩展选区（保留/建立锚点），否则清锚点。
    fn set_cursor(&mut self, idx: usize, extend: bool) {
        if extend {
            if self.state.sel_anchor.is_none() {
                self.state.sel_anchor = Some(self.state.cursor);
            }
        } else {
            self.state.sel_anchor = None;
        }
        self.state.cursor = idx.min(self.char_count());
    }

    /// 删除 [lo,hi) 字符；光标落到 lo，清锚点。
    fn delete_range(&mut self, lo: usize, hi: usize) {
        let mut chars: Vec<char> = self.base.text.chars().collect();
        let hi = hi.min(chars.len());
        let lo = lo.min(hi);
        if lo >= hi {
            self.state.sel_anchor = None;
            return;
        }
        chars.drain(lo..hi);
        self.base.text = chars.into_iter().collect();
        self.state.cursor = lo;
        self.state.sel_anchor = None;
        self.cache_dirty = true;
    }

    /// 在光标处插入字符串；若有选区则先替换掉选区。
    fn insert_str(&mut self, s: &str) -> bool {
        if self.config.read_only {
            return false;
        }
        let original_empty = s.is_empty();
        let s = self.accepted_input(s);
        let had_selection = self.sel_range().is_some();
        if !original_empty && s.is_empty() {
            return false;
        }
        if s.is_empty() && !had_selection {
            return false;
        }
        if let Some((lo, hi)) = self.sel_range() {
            self.delete_range(lo, hi);
        }
        let mut chars: Vec<char> = self.base.text.chars().collect();
        let idx = self.state.cursor.min(chars.len());
        let ins: Vec<char> = s.chars().collect();
        let n = ins.len();
        for (k, c) in ins.into_iter().enumerate() {
            chars.insert(idx + k, c);
        }
        self.base.text = chars.into_iter().collect();
        self.state.cursor = idx + n;
        self.state.sel_anchor = None;
        self.cache_dirty = true;
        true
    }

    /// 退格：有选区删选区，否则删光标前一个字符。
    fn backspace(&mut self) {
        if self.config.read_only {
            return;
        }
        if let Some((lo, hi)) = self.sel_range() {
            self.delete_range(lo, hi);
            return;
        }
        if self.state.cursor == 0 {
            return;
        }
        let idx = self.state.cursor - 1;
        self.delete_range(idx, idx + 1);
    }

    /// Delete：有选区删选区，否则删光标处字符。
    fn delete_forward(&mut self) {
        if self.config.read_only {
            return;
        }
        if let Some((lo, hi)) = self.sel_range() {
            self.delete_range(lo, hi);
            return;
        }
        let idx = self.state.cursor;
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
        let local_x = pos.x - content.left()
            + if self.config.multiline {
                0.0
            } else {
                self.scroll_x.get()
            };
        if self.lines.is_empty() {
            let cw = (self.base.font.size * 0.6).max(1.0);
            return ((local_x / cw).round().max(0.0) as usize).min(self.char_count());
        }
        let li = if self.config.multiline {
            let rel = (pos.y - content.top()) / self.line_height();
            (rel.floor().max(0.0) as usize).min(self.lines.len() - 1)
        } else {
            0
        };
        let col = self
            .col_at(li, local_x)
            .min(self.lines[li].offsets.len().saturating_sub(1));
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
        let (line, col) = self.pos_of(self.state.cursor);
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
            let style = self
                .config
                .placeholder_style
                .resolve(self.base.visual_state());
            let font = style.resolve_font(&self.base.font);
            let line_rect = Rect::new(content.left(), content.top(), content.size.width, line_h);
            draw_aligned_text(
                cv,
                &self.config.placeholder,
                line_rect,
                &font,
                style.fg_color.unwrap_or(PLACEHOLDER_COLOR),
                TextAlign::Left,
                false,
            );
            if self.base.focused && self.base.caret_on {
                let y = content.top() + (line_h - caret_h) / 2.0;
                cv.fill_rect(Rect::new(content.left(), y, 1.5, caret_h), color);
            }
            return;
        }
        let sel = self.sel_range();
        let (cur_line, cur_col) = self.pos_of(self.state.cursor);

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
                    let pre = self.display_slice(&line.chars().take(s - ls).collect::<String>());
                    let mid = self
                        .display_slice(&line.chars().skip(s - ls).take(e - s).collect::<String>());
                    let x0 = content.left() + cv.measure_text_advance(&pre, &self.base.font);
                    let w = cv.measure_text_advance(&mid, &self.base.font);
                    cv.fill_rect(Rect::new(x0, y, w.max(1.0), line_h), SEL_COLOR);
                }
            }
            // 文字。
            let line_rect = Rect::new(content.left(), y, content.size.width, line_h);
            Self::draw_input_text(
                cv,
                &self.display_slice(line),
                line_rect,
                &self.base.font,
                color,
            );
            // 光标（仅当前行）。
            if self.base.focused && self.base.caret_on && i == cur_line {
                let pre = self.display_slice(&line.chars().take(cur_col).collect::<String>());
                let cx = content.left() + cv.measure_text_advance(&pre, &self.base.font) + 1.0;
                let cyc = y + (line_h - caret_h) / 2.0;
                let caret = Rect::new(cx, cyc.max(y), 1.5, caret_h);
                self.caret_rect.set(Some(caret));
                cv.fill_rect(caret, color);
            }
            y += line_h;
            base_idx += ll + 1;
        }
    }

    fn shows_placeholder(&self) -> bool {
        self.base.text.is_empty()
            && self.state.marked.is_empty()
            && !self.config.placeholder.is_empty()
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
        cv.draw_text_advance(
            text,
            Point::new(rect.left(), y.max(rect.top())),
            font,
            color,
        );
    }

    fn update_single_line_scroll(&self, caret: f32, total: f32, content: Rect) {
        let width = content.size.width.max(0.0);
        if width <= 1.0 || total <= width {
            self.scroll_x.set(0.0);
            return;
        }
        let mut scroll = self.scroll_x.get();
        if caret < scroll {
            scroll = caret;
        } else if caret + 2.0 > scroll + width {
            scroll = caret + 2.0 - width;
        }
        self.scroll_x
            .set(scroll.clamp(0.0, (total + 2.0 - width).max(0.0)));
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
        if self.config.multiline {
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
            let display_line = self.display_slice(line);
            let mut offs = Vec::with_capacity(n + 1);
            for i in 0..=n {
                let pre: String = display_line.chars().take(i).collect();
                offs.push(cv.measure_text_advance(&pre, &self.base.font));
            }
            lines.push(LineCache {
                start,
                offsets: offs,
            });
            start += n + 1; // +1 跳过换行符
        }
        self.lines = lines;
        self.line_h = cv.measure_text("Ag", &self.base.font).height;
        self.cache_dirty = false;
        let content = layout::content_rect(&self.base);
        let (line, col) = self.pos_of(self.state.cursor);
        let x = self
            .lines
            .get(line)
            .and_then(|l| l.offsets.get(col))
            .copied()
            .unwrap_or(0.0);
        if self.config.multiline {
            self.caret_rect.set(Some(Rect::new(
                content.left() + x + 1.0,
                content.top() + line as f32 * self.line_h,
                1.5,
                self.base.font.size,
            )));
        } else {
            let total = self
                .lines
                .first()
                .and_then(|l| l.offsets.last())
                .copied()
                .unwrap_or(0.0);
            self.update_single_line_scroll(x, total, content);
            let y = content.top() + (content.size.height - self.base.font.size) / 2.0;
            self.caret_rect.set(Some(Rect::new(
                content.left() - self.scroll_x.get() + x + 1.0,
                y.max(content.top()),
                1.5,
                self.base.font.size,
            )));
        }
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        if self.config.multiline {
            self.paint_multiline(cv, style);
            return;
        }
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::BLACK);
        let caret_h = self.base.font.size;
        let cy = content.top() + (content.size.height - caret_h) / 2.0;
        if self.shows_placeholder() {
            self.scroll_x.set(0.0);
            self.caret_rect.set(Some(Rect::new(
                content.left(),
                cy.max(content.top()),
                1.5,
                caret_h,
            )));
            let style = self
                .config
                .placeholder_style
                .resolve(self.base.visual_state());
            let font = style.resolve_font(&self.base.font);
            draw_aligned_text(
                cv,
                &self.config.placeholder,
                content,
                &font,
                style.fg_color.unwrap_or(PLACEHOLDER_COLOR),
                TextAlign::Left,
                false,
            );
            if self.base.focused && self.base.caret_on {
                cv.fill_rect(
                    Rect::new(content.left(), cy.max(content.top()), 1.5, caret_h),
                    color,
                );
            }
            return;
        }
        let before: String = self.base.text.chars().take(self.state.cursor).collect();
        let after: String = self.base.text.chars().skip(self.state.cursor).collect();
        let display_before = self.display_slice(&before);
        let display_after = self.display_slice(&after);
        let display_marked = self.display_slice(&self.state.marked);
        let display = format!("{display_before}{display_marked}{display_after}");
        let before_w = cv.measure_text_advance(&display_before, &self.base.font);
        let marked_w = cv.measure_text_advance(&display_marked, &self.base.font);
        let total_w = cv.measure_text_advance(&display, &self.base.font);
        self.update_single_line_scroll(before_w + marked_w, total_w, content);
        let origin_x = content.left() - self.scroll_x.get();
        cv.save();
        cv.clip_rect(content);
        // 选区高亮（组合中不画）：先在文字底下铺一条半透明矩形。
        if self.state.marked.is_empty() {
            if let Some((lo, hi)) = self.sel_range() {
                let pre = self.display_slice(&self.base.text.chars().take(lo).collect::<String>());
                let sel = self.display_slice(
                    &self
                        .base
                        .text
                        .chars()
                        .skip(lo)
                        .take(hi - lo)
                        .collect::<String>(),
                );
                let x0 = origin_x + cv.measure_text_advance(&pre, &self.base.font);
                let w = cv.measure_text_advance(&sel, &self.base.font);
                cv.fill_rect(
                    Rect::new(x0, cy.max(content.top()), w.max(1.0), caret_h),
                    SEL_COLOR,
                );
            }
        }
        // 显示串 = 光标前文本 + IME 组合串 + 光标后文本；组合串加下划线以示未提交。
        let text_rect = Rect::new(
            origin_x,
            content.top(),
            total_w.max(content.size.width),
            content.size.height,
        );
        Self::draw_input_text(cv, &display, text_rect, &self.base.font, color);

        // 组合串下划线。
        if !self.state.marked.is_empty() {
            let uy = (cy + caret_h - 1.0).min(content.bottom() - 1.0);
            cv.fill_rect(
                Rect::new(origin_x + before_w, uy, marked_w.max(1.0), 1.0),
                color,
            );
        }
        // 仅在获得焦点且闪烁相位为亮时画光标；光标落在组合串之后；高度与字号一致、垂直居中。
        let caret = Rect::new(
            origin_x + before_w + marked_w + 1.0,
            cy.max(content.top()),
            1.5,
            caret_h,
        );
        self.caret_rect.set(Some(caret));
        if self.base.focused && self.base.caret_on {
            cv.fill_rect(caret, color);
        }
        cv.restore();
    }
    fn on_event(&mut self, ev: &Event) -> EventFlow {
        match ev {
            Event::Char { ch } if !ch.is_control() => {
                if self.insert_str(&ch.to_string()) {
                    EventFlow::Consumed
                } else {
                    EventFlow::Ignored
                }
            }
            // 鼠标按下：光标定位到点击处，并把锚点也置于此（拖动即从这里起选）。
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
            } => {
                let idx = self.hit_index(*pos);
                self.state.cursor = idx;
                self.state.sel_anchor = Some(idx);
                EventFlow::Consumed
            }
            // 鼠标拖动（分发器仅在按住本控件时转发）：延伸选区到当前位置。
            Event::MouseMove { pos } => {
                self.state.cursor = self.hit_index(*pos);
                EventFlow::Consumed
            }
            // 双击：选中光标处的词。
            Event::DoubleClick { pos } => {
                let idx = self.hit_index(*pos);
                let (start, end) = self.word_range_at(idx);
                self.state.sel_anchor = Some(start);
                self.state.cursor = end;
                EventFlow::Consumed
            }
            Event::KeyDown { key, mods } => match *key {
                keys::BACKSPACE => {
                    if self.config.read_only {
                        return EventFlow::Ignored;
                    }
                    self.backspace();
                    EventFlow::Consumed
                }
                keys::DELETE => {
                    if self.config.read_only {
                        return EventFlow::Ignored;
                    }
                    self.delete_forward();
                    EventFlow::Consumed
                }
                keys::LEFT => {
                    // 有选区且不按 Shift：收起到选区左端；否则左移（Shift 扩展）。
                    if !mods.shift {
                        if let Some((lo, _)) = self.sel_range() {
                            self.state.cursor = lo;
                            self.state.sel_anchor = None;
                        } else {
                            self.set_cursor(self.state.cursor.saturating_sub(1), false);
                        }
                    } else {
                        self.set_cursor(self.state.cursor.saturating_sub(1), true);
                    }
                    EventFlow::Consumed
                }
                keys::RIGHT => {
                    if !mods.shift {
                        if let Some((_, hi)) = self.sel_range() {
                            self.state.cursor = hi;
                            self.state.sel_anchor = None;
                        } else {
                            self.set_cursor(self.state.cursor + 1, false);
                        }
                    } else {
                        self.set_cursor(self.state.cursor + 1, true);
                    }
                    EventFlow::Consumed
                }
                keys::HOME => {
                    // 多行：到本行行首；单行：到文本开头。
                    let target = if self.config.multiline && !self.lines.is_empty() {
                        let (line, _) = self.pos_of(self.state.cursor);
                        self.lines[line].start
                    } else {
                        0
                    };
                    self.set_cursor(target, mods.shift);
                    EventFlow::Consumed
                }
                keys::END => {
                    let target = if self.config.multiline && !self.lines.is_empty() {
                        let (line, _) = self.pos_of(self.state.cursor);
                        let len = self.lines[line].offsets.len().saturating_sub(1);
                        self.lines[line].start + len
                    } else {
                        self.char_count()
                    };
                    self.set_cursor(target, mods.shift);
                    EventFlow::Consumed
                }
                keys::ENTER if self.config.multiline => {
                    if self.insert_str("\n") {
                        EventFlow::Consumed
                    } else {
                        EventFlow::Ignored
                    }
                }
                keys::UP if self.config.multiline => {
                    self.move_vertical(-1, mods.shift);
                    EventFlow::Consumed
                }
                keys::DOWN if self.config.multiline => {
                    self.move_vertical(1, mods.shift);
                    EventFlow::Consumed
                }
                _ => EventFlow::Ignored,
            },
            _ => EventFlow::Ignored,
        }
    }

    fn selected_text(&self) -> Option<String> {
        if self.config.password {
            return None;
        }
        let (lo, hi) = self.sel_range()?;
        Some(self.base.text.chars().skip(lo).take(hi - lo).collect())
    }
    fn replace_selection(&mut self, s: &str) -> bool {
        self.insert_str(s)
    }
    fn delete_selection(&mut self) -> bool {
        if self.config.read_only || self.config.password {
            return false;
        }
        if let Some((lo, hi)) = self.sel_range() {
            self.delete_range(lo, hi);
            true
        } else {
            false
        }
    }
    fn select_all(&mut self) {
        self.state.sel_anchor = Some(0);
        self.state.cursor = self.char_count();
    }

    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Placeholder(v) => self.config.placeholder = v,
            WidgetProperty::PlaceholderStyle(v) => self.config.placeholder_style = v,
            WidgetProperty::Multiline(v) => self.config.multiline = v,
            WidgetProperty::ReadOnly(v) => self.config.read_only = v,
            WidgetProperty::NumberOnly(v) => {
                self.config.number_only = v;
                self.normalize_text();
            }
            WidgetProperty::Password(v) => {
                self.config.password = v;
                self.cache_dirty = true;
            }
            WidgetProperty::PasswordChar(v) => {
                self.config.password_char = v;
                self.cache_dirty = true;
            }
            WidgetProperty::MaxChars(v) => {
                self.config.max_chars = v;
                self.normalize_text();
            }
            WidgetProperty::AutoSelectAll(v) => self.config.auto_select_all = v,
            _ => return false,
        }
        true
    }

    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        match key {
            WidgetPropertyKey::Placeholder => {
                Some(WidgetProperty::Placeholder(self.config.placeholder.clone()))
            }
            WidgetPropertyKey::PlaceholderStyle => Some(WidgetProperty::PlaceholderStyle(
                self.config.placeholder_style.clone(),
            )),
            WidgetPropertyKey::Multiline => Some(WidgetProperty::Multiline(self.config.multiline)),
            WidgetPropertyKey::ReadOnly => Some(WidgetProperty::ReadOnly(self.config.read_only)),
            WidgetPropertyKey::NumberOnly => {
                Some(WidgetProperty::NumberOnly(self.config.number_only))
            }
            WidgetPropertyKey::Password => Some(WidgetProperty::Password(self.config.password)),
            WidgetPropertyKey::PasswordChar => {
                Some(WidgetProperty::PasswordChar(self.config.password_char))
            }
            WidgetPropertyKey::MaxChars => Some(WidgetProperty::MaxChars(self.config.max_chars)),
            WidgetPropertyKey::AutoSelectAll => {
                Some(WidgetProperty::AutoSelectAll(self.config.auto_select_all))
            }
            _ => None,
        }
    }

    fn focus_gained(&mut self) {
        if self.config.auto_select_all {
            self.select_all();
        }
    }
    fn set_text_value(&mut self, text: String) {
        self.base.text = text;
        self.state.cursor = self.base.text.chars().count();
        self.state.sel_anchor = None;
        self.state.marked.clear();
        self.normalize_text();
    }
    fn text_input_rect(&self) -> Option<Rect> {
        self.caret_rect.get().or(Some(self.base.rect))
    }
    fn text_input_state(&self) -> Option<TextInputState> {
        Some(TextInputState {
            text: self.base.text.clone(),
            cursor: self.state.cursor,
            selection: self.sel_range(),
            marked: self.state.marked.clone(),
        })
    }
    fn set_marked_text(&mut self, text: String) -> bool {
        if self.config.read_only {
            return false;
        }
        self.state.marked = text;
        true
    }
    fn clear_marked_text(&mut self) -> bool {
        if self.state.marked.is_empty() {
            false
        } else {
            self.state.marked.clear();
            true
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
        e.state.cursor = 1;
        e.set_marked_text("b".to_string());
        e.base_mut().focused = true;
        let cv = RecCanvas {
            last_text: RefCell::new(String::new()),
            last_font: RefCell::new(None),
            last_color: RefCell::new(None),
        };
        let mut cv = cv;
        layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        let style = StyleSpec::default();
        e.paint_content(&mut cv, &style);
        assert_eq!(*cv.last_text.borrow(), "abc");
        assert_eq!(e.base().text, "ac", "组合串不改动已提交文本");
    }

    fn rec_canvas() -> RecCanvas {
        RecCanvas {
            last_text: RefCell::new(String::new()),
            last_font: RefCell::new(None),
            last_color: RefCell::new(None),
        }
    }

    use crate::event::{keys, Event, Mods};

    fn kd(key: u32, shift: bool) -> Event {
        Event::KeyDown {
            key,
            mods: Mods {
                shift,
                ..Default::default()
            },
        }
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
            font_family: Some("Microsoft YaHei".to_string()),
            font_size: Some(13.0),
            fg_color: Some(Color::WHITE),
            bold: Some(true),
            italic: Some(false),
            underline: Some(true),
        });
        styles.set(
            VisualState::new(BaseState::Hot, false),
            PlaceholderStyleSpec {
                font_size: Some(15.0),
                fg_color: Some(Color::BLACK),
                italic: Some(true),
                ..Default::default()
            },
        );
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
        assert_eq!(e.selection(), Some((3, 5)));
        assert_eq!(e.selected_text().as_deref(), Some("lo"));
        // 平移 Left：收起到左端 3，无选区
        e.on_event(&kd(keys::LEFT, false));
        assert_eq!(e.cursor(), 3);
        assert_eq!(e.selection(), None);
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
        assert_eq!(e.cursor(), 1);
        assert_eq!(e.selection(), None);
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
        assert_eq!(e.cursor(), 2);
        // 下移回第2行同列(col2 → 索引5)。
        e.on_event(&kd(keys::DOWN, false));
        assert_eq!(e.cursor(), 5);
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
        assert_eq!(e.selection(), None, "锚点与光标重合视为无选区");
        assert_eq!(e.selected_text(), None);
    }

    #[test]
    fn 输入约束统一作用于键盘和粘贴() {
        let mut e = Edit::new().number_only(true).max_chars(4);
        assert!(e.replace_selection("1a23中45"));
        assert_eq!(e.base().text, "1234");
        assert_eq!(e.on_event(&Event::Char { ch: '6' }), EventFlow::Ignored);
        e.select_all();
        assert!(!e.replace_selection("abc"));
        assert_eq!(e.base().text, "1234", "被过滤的输入不应删除当前选区");
    }

    #[test]
    fn 只读与密码保护剪贴板内容() {
        let mut readonly = Edit::new().text("hello").read_only(true);
        readonly.select_all();
        assert_eq!(readonly.selected_text().as_deref(), Some("hello"));
        assert!(!readonly.replace_selection("x"));
        let mut password = Edit::new().text("secret").password(true);
        password.select_all();
        assert_eq!(password.selected_text(), None);
        assert!(!password.delete_selection());
    }

    #[test]
    fn 长文本滚动后光标保持可见且命中考虑滚动() {
        let cv = FakeCanvas;
        let mut e = Edit::new().text("abcdefghij");
        layout_node(&mut e, Rect::new(0.0, 0.0, 40.0, 30.0), &cv);
        assert!(e.scroll_x.get() > 0.0);
        let content = layout::content_rect(e.base());
        let caret = e.text_input_rect().unwrap();
        assert!(caret.left() >= content.left() && caret.right() <= content.right() + 2.0);
        assert!(e.hit_index(Point::new(content.right() - 1.0, content.top() + 2.0)) >= 8);
    }
}
