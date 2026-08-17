//! Edit：文本输入（选区、复制/剪切/粘贴、IME；单行/多行）。

use std::cell::Cell;

use flexui_geometry::{Color, Point, Rect, Size};
use flexui_gfx::{Canvas, TextLayout};
use unicode_segmentation::UnicodeSegmentation;

use crate::anim::AnimProp;
use crate::common_builders;
use crate::event::{keys, Event, EventFlow, MouseButton};
use crate::layout;
use crate::scroll::{paint_scrollbars, ScrollAxes, ScrollBarStyle, ScrollState};
use crate::style::{PlaceholderStyleSet, StyleSpec};
use crate::theme::WidgetKind;
use crate::widget::{
    Base, TextControl, TextInputState, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole,
};

/// 选区高亮色（半透明蓝）。
const SEL_COLOR: Color = Color::rgba(0.20, 0.45, 0.95, 0.35);
const PLACEHOLDER_COLOR: Color = Color::rgba(0.50, 0.50, 0.50, 1.0);
const CARET_WIDTH: f32 = 1.0;

/// 逻辑行缓存；排版、绘制、光标与命中测试共用同一个平台文字布局。
struct LineCache {
    start: usize,
    char_len: usize,
    grapheme_boundaries: Vec<usize>,
    layout: TextLayout,
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
    /// 单行显示布局，包含插入到光标处的 IME marked text。
    display_layout: Option<TextLayout>,
    /// 占位文本布局；字体随当前视觉状态解析。
    placeholder_layout: Option<TextLayout>,
    /// 行高（measure("Ag").height），arrange 时算出，供多行光标/命中定位。
    line_h: f32,
    /// 缓存是否因文本变更而过期（过期时映射回退到等宽估算）。
    cache_dirty: bool,
    /// 统一滚动状态：单行走横向（`offset.x`），多行走纵向（`offset.y`）。
    /// 用 `Cell` 以便 `&self` 的绘制阶段随光标跟随更新。
    scroll: Cell<ScrollState>,
    /// 纵向滚动条外观（多行内容超出视口时绘制）。
    scrollbar: ScrollBarStyle,
    /// 最近一次排版/绘制得到的真实插入点矩形。
    caret_rect: Cell<Option<Rect>>,
}

impl Edit {
    pub fn new() -> Self {
        Self {
            base: Base::new_kind(WidgetRole::Edit, WidgetKind::Edit),
            config: EditConfig::default(),
            state: EditState::default(),
            lines: Vec::new(),
            display_layout: None,
            placeholder_layout: None,
            line_h: 0.0,
            cache_dirty: true,
            scroll: Cell::new(ScrollState::new(ScrollAxes::both())),
            scrollbar: ScrollBarStyle::default(),
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

    fn char_to_byte(text: &str, char_index: usize) -> usize {
        text.char_indices()
            .nth(char_index)
            .map_or(text.len(), |(byte, _)| byte)
    }

    fn byte_to_char(text: &str, byte_index: usize) -> usize {
        text[..byte_index.min(text.len())].chars().count()
    }

    fn previous_grapheme_boundary(&self, char_index: usize) -> usize {
        let byte_index = Self::char_to_byte(&self.base.text, char_index);
        self.base
            .text
            .grapheme_indices(true)
            .rev()
            .find_map(|(byte, _)| {
                (byte < byte_index).then_some(Self::byte_to_char(&self.base.text, byte))
            })
            .unwrap_or(0)
    }

    fn next_grapheme_boundary(&self, char_index: usize) -> usize {
        let byte_index = Self::char_to_byte(&self.base.text, char_index);
        self.base
            .text
            .grapheme_indices(true)
            .find_map(|(byte, _)| {
                (byte > byte_index).then_some(Self::byte_to_char(&self.base.text, byte))
            })
            .unwrap_or_else(|| self.char_count())
    }

    fn grapheme_char_boundaries(text: &str) -> Vec<usize> {
        let mut boundaries = vec![0];
        let mut char_index = 0;
        for grapheme in text.graphemes(true) {
            char_index += grapheme.chars().count();
            boundaries.push(char_index);
        }
        boundaries
    }

    fn snap_to_grapheme_boundary(
        raw_index: usize,
        target_x: f32,
        boundaries: &[usize],
        x_for_char: impl Fn(usize) -> f32,
    ) -> usize {
        match boundaries.binary_search(&raw_index) {
            Ok(index) => boundaries[index],
            Err(index) => {
                let before = boundaries[index.saturating_sub(1)];
                let after = boundaries.get(index).copied().unwrap_or(before);
                if (x_for_char(before) - target_x).abs() <= (x_for_char(after) - target_x).abs() {
                    before
                } else {
                    after
                }
            }
        }
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
        let idx = self.previous_grapheme_boundary(self.state.cursor);
        self.delete_range(idx, self.state.cursor);
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
        self.delete_range(idx, self.next_grapheme_boundary(idx));
    }

    /// 行内本地 x → 最近 grapheme 边界列号（0..=行长）。
    fn col_at(&self, li: usize, local_x: f32) -> usize {
        let est = |x: f32| ((x / (self.base.font.size * 0.6).max(1.0)).round().max(0.0)) as usize;
        if self.cache_dirty || li >= self.lines.len() {
            return est(local_x);
        }
        let line = &self.lines[li];
        let raw = line.layout.closest_char_for_x(local_x);
        Self::snap_to_grapheme_boundary(raw, local_x, &line.grapheme_boundaries, |index| {
            line.layout.x_for_char(index)
        })
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
        let offset = self.scroll.get().offset();
        let local_x = pos.x - content.left()
            + if self.config.multiline {
                0.0
            } else {
                offset.x
            };
        if !self.config.multiline && !self.cache_dirty {
            if let Some(layout) = &self.display_layout {
                let display_index = layout.closest_char_for_x(local_x);
                let marked_len = self.state.marked.chars().count();
                let raw_index = if display_index <= self.state.cursor {
                    display_index
                } else if display_index <= self.state.cursor + marked_len {
                    self.state.cursor
                } else {
                    display_index.saturating_sub(marked_len)
                };
                let Some(line) = self.lines.first() else {
                    return raw_index.min(self.char_count());
                };
                return Self::snap_to_grapheme_boundary(
                    raw_index.min(self.char_count()),
                    local_x,
                    &line.grapheme_boundaries,
                    |index| {
                        if index < self.state.cursor {
                            layout.x_for_char(index)
                        } else if index > self.state.cursor {
                            layout.x_for_char(index + marked_len)
                        } else {
                            let before = layout.x_for_char(index);
                            let after = layout.x_for_char(index + marked_len);
                            if (before - local_x).abs() <= (after - local_x).abs() {
                                before
                            } else {
                                after
                            }
                        }
                    },
                );
            }
        }
        if self.lines.is_empty() {
            let cw = (self.base.font.size * 0.6).max(1.0);
            return ((local_x / cw).round().max(0.0) as usize).min(self.char_count());
        }
        let li = if self.config.multiline {
            let rel = (pos.y - content.top() + offset.y) / self.line_height();
            (rel.floor().max(0.0) as usize).min(self.lines.len() - 1)
        } else {
            0
        };
        let col = self.col_at(li, local_x).min(self.lines[li].char_len);
        self.lines[li].start + col
    }

    /// 字符索引 → (行号, 列号)。缓存缺失时回退 (0, idx)。
    fn pos_of(&self, idx: usize) -> (usize, usize) {
        for (i, l) in self.lines.iter().enumerate() {
            if idx <= l.start + l.char_len {
                return (i, idx - l.start);
            }
        }
        match self.lines.last() {
            Some(l) => (self.lines.len() - 1, l.char_len),
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
        let x = self.lines[line].layout.x_for_char(col);
        let target_line = &self.lines[target];
        let raw_target_col = target_line.layout.closest_char_for_x(x);
        let target_col = Self::snap_to_grapheme_boundary(
            raw_target_col,
            x,
            &target_line.grapheme_boundaries,
            |index| target_line.layout.x_for_char(index),
        );
        let nidx = self.lines[target].start + target_col.min(self.lines[target].char_len);
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
        let caret_h = line_h.max(self.base.font.size);
        if self.shows_placeholder() {
            let style = self
                .config
                .placeholder_style
                .resolve(self.base.visual_state());
            let font = style.resolve_font(&self.base.font);
            let generated;
            let placeholder =
                match self.placeholder_layout.as_ref().filter(|layout| {
                    layout.text() == self.config.placeholder && layout.font() == &font
                }) {
                    Some(layout) => layout,
                    None => {
                        generated = cv.layout_text(&self.config.placeholder, &font);
                        &generated
                    }
                };
            cv.save();
            cv.clip_rect(self.text_clip_rect(content));
            Self::draw_input_layout(
                cv,
                placeholder,
                Rect::new(content.left(), content.top(), content.size.width, line_h),
                style
                    .fg_color
                    .or_else(|| self.base.resolved_style().placeholder_color)
                    .unwrap_or(PLACEHOLDER_COLOR),
            );
            if self.base.focused && self.base.caret_on {
                let y = content.top() + (line_h - caret_h) / 2.0;
                cv.fill_rect(Rect::new(content.left(), y, CARET_WIDTH, caret_h), color);
            }
            cv.restore();
            return;
        }
        let sel = self.sel_range();
        let (cur_line, cur_col) = self.pos_of(self.state.cursor);

        let offset_y = self.scroll.get().offset().y;
        cv.save();
        cv.clip_rect(self.text_clip_rect(content));
        let mut y = content.top() - offset_y;
        for (i, line) in self.lines.iter().enumerate() {
            // 视口外的行跳过绘制（仍需累加 y）。
            if y + line_h < content.top() || y > content.bottom() {
                y += line_h;
                continue;
            }
            if let Some((lo, hi)) = sel {
                let (ls, le) = (line.start, line.start + line.char_len);
                let s = lo.max(ls);
                let e = hi.min(le);
                if s < e {
                    for rect in line.layout.selection_rects(s - ls..e - ls, y, line_h) {
                        cv.fill_rect(
                            Rect::new(
                                content.left() + rect.left(),
                                rect.top(),
                                rect.size.width,
                                rect.size.height,
                            ),
                            SEL_COLOR,
                        );
                    }
                }
            }
            Self::draw_input_layout(
                cv,
                &line.layout,
                Rect::new(content.left(), y, content.size.width, line_h),
                color,
            );
            if self.base.focused && self.base.caret_on && i == cur_line {
                let cx = content.left() + line.layout.x_for_char(cur_col) + 1.0;
                let cyc = y + (line_h - caret_h) / 2.0;
                let caret = Rect::new(cx, cyc.max(y), CARET_WIDTH, caret_h);
                self.caret_rect.set(Some(caret));
                cv.fill_rect(caret, color);
            }
            y += line_h;
        }
        cv.restore();
        // 内容超出视口时绘制纵向滚动条。
        let state = self.scroll.get();
        paint_scrollbars(cv, content, &state, &self.scrollbar, style);
    }

    fn shows_placeholder(&self) -> bool {
        self.base.text.is_empty()
            && self.state.marked.is_empty()
            && !self.config.placeholder.is_empty()
    }

    /// 输入文本直接绘制 shaping 结果，确保像素与交互边界来自同一次排版。
    fn draw_input_layout(cv: &mut dyn Canvas, layout: &TextLayout, rect: Rect, color: Color) {
        if layout.text().is_empty() {
            return;
        }
        cv.draw_text_layout(
            layout,
            Point::new(rect.left(), Self::layout_y(layout, rect)),
            color,
        );
    }

    fn layout_y(layout: &TextLayout, rect: Rect) -> f32 {
        (rect.top() + (rect.size.height - layout.height()) / 2.0).max(rect.top())
    }

    fn update_single_line_scroll(&self, caret: f32, total: f32, content: Rect) {
        let mut state = self.scroll.get();
        // 单行内容尺寸：宽 = 文本总宽 + 光标留白，高 = 视口高（纵向不滚）。
        state.set_metrics(
            Size::new(total + 2.0, content.size.height),
            content.size,
        );
        // 让光标（含 2px 留白）保持在视口内。
        state.ensure_visible(
            Rect::new(caret, 0.0, CARET_WIDTH + 2.0, content.size.height),
            0.0,
        );
        self.scroll.set(state);
    }

    /// 横向严格裁到内容区；纵向保留控件 padding，避免字体抗锯齿下沿被切掉。
    fn text_clip_rect(&self, content: Rect) -> Rect {
        Rect::new(
            content.left(),
            self.base.rect.top(),
            content.size.width,
            self.base.rect.size.height,
        )
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
        let s = cv.layout_text("Ag", &self.base.font).size();
        if self.config.multiline {
            let rows = self.base.text.split('\n').count().max(1) as f32;
            layout::size_from_content(&self.base, 120.0, rows * s.height + 8.0)
        } else {
            layout::size_from_content(&self.base, 120.0, s.height + 8.0)
        }
    }
    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
        layout::arrange_stack(&mut self.base, content, cv);
        let text = self.base.text.clone();
        let mut lines = Vec::new();
        let mut start = 0usize;
        for line in text.split('\n') {
            let n = line.chars().count();
            let display_line = self.display_slice(line);
            lines.push(LineCache {
                start,
                char_len: n,
                grapheme_boundaries: Self::grapheme_char_boundaries(line),
                layout: cv.layout_text(&display_line, &self.base.font),
            });
            start += n + 1; // +1 跳过换行符
        }
        self.lines = lines;
        self.line_h = cv.layout_text("Ag", &self.base.font).height();

        self.display_layout = if self.config.multiline {
            None
        } else {
            let before = self
                .base
                .text
                .chars()
                .take(self.state.cursor)
                .collect::<String>();
            let after = self
                .base
                .text
                .chars()
                .skip(self.state.cursor)
                .collect::<String>();
            let display = format!(
                "{}{}{}",
                self.display_slice(&before),
                self.display_slice(&self.state.marked),
                self.display_slice(&after)
            );
            Some(cv.layout_text(&display, &self.base.font))
        };
        let placeholder_style = self
            .config
            .placeholder_style
            .resolve(self.base.visual_state());
        let placeholder_font = placeholder_style.resolve_font(&self.base.font);
        self.placeholder_layout = (!self.config.placeholder.is_empty())
            .then(|| cv.layout_text(&self.config.placeholder, &placeholder_font));
        self.cache_dirty = false;
        let content = layout::content_rect(&self.base);
        let (line, col) = self.pos_of(self.state.cursor);
        let x = self
            .lines
            .get(line)
            .map(|line| line.layout.x_for_char(col))
            .unwrap_or(0.0);
        if self.config.multiline {
            // 更新纵向滚动度量并让光标行进入视口。
            let mut state = self.scroll.get();
            let total_h = self.lines.len() as f32 * self.line_h;
            state.set_metrics(Size::new(content.size.width, total_h), content.size);
            state.ensure_visible(
                Rect::new(x, line as f32 * self.line_h, CARET_WIDTH, self.line_h),
                0.0,
            );
            self.scroll.set(state);
            let offset_y = state.offset().y;
            self.caret_rect.set(Some(Rect::new(
                content.left() + x + 1.0,
                content.top() + line as f32 * self.line_h - offset_y,
                CARET_WIDTH,
                self.base.font.size,
            )));
        } else {
            let total = self
                .display_layout
                .as_ref()
                .map(TextLayout::width)
                .unwrap_or(0.0);
            let display_cursor = self.state.cursor + self.state.marked.chars().count();
            let display_x = self
                .display_layout
                .as_ref()
                .map(|layout| layout.x_for_char(display_cursor))
                .unwrap_or(x);
            self.update_single_line_scroll(display_x, total, content);
            let caret_h = self
                .display_layout
                .as_ref()
                .map(TextLayout::height)
                .unwrap_or(self.line_h.max(self.base.font.size));
            let y = (content.top() + (content.size.height - caret_h) / 2.0).max(content.top());
            self.caret_rect.set(Some(Rect::new(
                content.left() - self.scroll.get().offset().x + display_x + 1.0,
                y,
                CARET_WIDTH,
                caret_h,
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
        if self.shows_placeholder() {
            let mut state = self.scroll.get();
            state.set_offset(0.0, 0.0);
            self.scroll.set(state);
            let style = self
                .config
                .placeholder_style
                .resolve(self.base.visual_state());
            let font = style.resolve_font(&self.base.font);
            let generated;
            let placeholder =
                match self.placeholder_layout.as_ref().filter(|layout| {
                    layout.text() == self.config.placeholder && layout.font() == &font
                }) {
                    Some(layout) => layout,
                    None => {
                        generated = cv.layout_text(&self.config.placeholder, &font);
                        &generated
                    }
                };
            let line_h = placeholder.height();
            let line_y = Self::layout_y(placeholder, content);
            self.caret_rect
                .set(Some(Rect::new(content.left(), line_y, CARET_WIDTH, line_h)));
            cv.save();
            cv.clip_rect(self.text_clip_rect(content));
            Self::draw_input_layout(
                cv,
                placeholder,
                content,
                style
                    .fg_color
                    .or_else(|| self.base.resolved_style().placeholder_color)
                    .unwrap_or(PLACEHOLDER_COLOR),
            );
            if self.base.focused && self.base.caret_on {
                cv.fill_rect(
                    Rect::new(content.left(), line_y, CARET_WIDTH, line_h),
                    color,
                );
            }
            cv.restore();
            return;
        }
        let before: String = self.base.text.chars().take(self.state.cursor).collect();
        let after: String = self.base.text.chars().skip(self.state.cursor).collect();
        let display_before = self.display_slice(&before);
        let display_after = self.display_slice(&after);
        let display_marked = self.display_slice(&self.state.marked);
        let display = format!("{display_before}{display_marked}{display_after}");
        let generated;
        let text_layout = match self
            .display_layout
            .as_ref()
            .filter(|layout| layout.text() == display && layout.font() == &self.base.font)
        {
            Some(layout) => layout,
            None => {
                generated = cv.layout_text(&display, &self.base.font);
                &generated
            }
        };
        let marked_chars = display_marked.chars().count();
        let before_w = text_layout.x_for_char(self.state.cursor);
        let marked_end_w = text_layout.x_for_char(self.state.cursor + marked_chars);
        let total_w = text_layout.width();
        let line_h = text_layout.height();
        let line_y = Self::layout_y(text_layout, content);
        self.update_single_line_scroll(marked_end_w, total_w, content);
        let origin_x = content.left() - self.scroll.get().offset().x;
        cv.save();
        cv.clip_rect(self.text_clip_rect(content));
        // 选区高亮（组合中不画）：先在文字底下铺一条半透明矩形。
        if self.state.marked.is_empty() {
            if let Some((lo, hi)) = self.sel_range() {
                for rect in text_layout.selection_rects(lo..hi, line_y, line_h) {
                    cv.fill_rect(
                        Rect::new(
                            origin_x + rect.left(),
                            rect.top(),
                            rect.size.width,
                            rect.size.height,
                        ),
                        SEL_COLOR,
                    );
                }
            }
        }
        // 显示串 = 光标前文本 + IME 组合串 + 光标后文本；组合串加下划线以示未提交。
        let text_rect = Rect::new(
            origin_x,
            content.top(),
            total_w.max(content.size.width),
            content.size.height,
        );
        Self::draw_input_layout(cv, text_layout, text_rect, color);

        // 组合串下划线。
        if !self.state.marked.is_empty() {
            let uy = (line_y + line_h - 1.0).min(content.bottom() - 1.0);
            let left = before_w.min(marked_end_w);
            let right = before_w.max(marked_end_w);
            cv.fill_rect(
                Rect::new(origin_x + left, uy, (right - left).max(1.0), 1.0),
                color,
            );
        }
        // 仅在获得焦点且闪烁相位为亮时画光标；光标落在组合串之后；高度与排版行一致、垂直居中。
        let caret = Rect::new(origin_x + marked_end_w + 1.0, line_y, CARET_WIDTH, line_h);
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
                            self.set_cursor(
                                self.previous_grapheme_boundary(self.state.cursor),
                                false,
                            );
                        }
                    } else {
                        self.set_cursor(self.previous_grapheme_boundary(self.state.cursor), true);
                    }
                    EventFlow::Consumed
                }
                keys::RIGHT => {
                    if !mods.shift {
                        if let Some((_, hi)) = self.sel_range() {
                            self.state.cursor = hi;
                            self.state.sel_anchor = None;
                        } else {
                            self.set_cursor(self.next_grapheme_boundary(self.state.cursor), false);
                        }
                    } else {
                        self.set_cursor(self.next_grapheme_boundary(self.state.cursor), true);
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
                        self.lines[line].start + self.lines[line].char_len
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
            WidgetProperty::Placeholder(v) => {
                self.config.placeholder = v;
                self.cache_dirty = true;
            }
            WidgetProperty::PlaceholderStyle(v) => {
                self.config.placeholder_style = v;
                self.cache_dirty = true;
            }
            WidgetProperty::Multiline(v) => {
                self.config.multiline = v;
                self.cache_dirty = true;
            }
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
    fn is_scrollable(&self) -> bool {
        // 仅多行且内容超出视口时接收滚轮（单行靠光标跟随横向卷动）。
        self.config.multiline && self.scroll.get().needs_v()
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let mut state = self.scroll.get();
        let changed = state.scroll_by(dx, dy);
        self.scroll.set(state);
        changed
    }
    fn scroll_offset(&self) -> Option<Point> {
        Some(self.scroll.get().offset())
    }
    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        self.scroll.get().axis_value(prop)
    }
    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        let mut state = self.scroll.get();
        let handled = state.set_axis_value(prop, value);
        self.scroll.set(state);
        handled
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
        self.cache_dirty = true;
        true
    }
    fn clear_marked_text(&mut self) -> bool {
        if self.state.marked.is_empty() {
            false
        } else {
            self.state.marked.clear();
            self.cache_dirty = true;
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
        fills: Vec<(Rect, Color)>,
        advance_draws: usize,
    }
    impl Canvas for RecCanvas {
        fn fill_rect(&mut self, r: Rect, c: Color) {
            self.fills.push((r, c));
        }
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, t: &str, _o: Point, f: &Font, c: Color) {
            *self.last_text.borrow_mut() = t.to_string();
            *self.last_font.borrow_mut() = Some(f.clone());
            *self.last_color.borrow_mut() = Some(c);
        }
        fn draw_text_advance(&mut self, t: &str, o: Point, f: &Font, c: Color) {
            self.advance_draws += 1;
            self.draw_text(t, o, f, c);
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
            fills: Vec::new(),
            advance_draws: 0,
        };
        let mut cv = cv;
        layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        let style = StyleSpec::default();
        e.paint_content(&mut cv, &style);
        assert_eq!(*cv.last_text.borrow(), "abc");
        assert_eq!(cv.advance_draws, 1);
        assert_eq!(e.base().text, "ac", "组合串不改动已提交文本");
    }

    fn rec_canvas() -> RecCanvas {
        RecCanvas {
            last_text: RefCell::new(String::new()),
            last_font: RefCell::new(None),
            last_color: RefCell::new(None),
            fills: Vec::new(),
            advance_draws: 0,
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
        assert_eq!(cv.advance_draws, 1, "占位文本必须使用输入文本排版路径");
        assert!(edit.base().text.is_empty(), "占位文本不能成为输入内容");
        edit.on_event(&Event::Char { ch: 'A' });
        edit.paint_content(&mut cv, &StyleSpec::default());
        assert_eq!(*cv.last_text.borrow(), "A");
        edit.on_event(&kd(keys::BACKSPACE, false));
        edit.paint_content(&mut cv, &StyleSpec::default());
        assert_eq!(*cv.last_text.borrow(), "请输入");
    }

    #[test]
    fn 单行选区和光标覆盖实际排版行高() {
        let mut edit = Edit::new().text("fjord你好");
        edit.select_all();
        edit.base_mut().focused = true;
        let mut cv = rec_canvas();
        layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        edit.paint_content(&mut cv, &StyleSpec::default());

        let layout = edit.display_layout.as_ref().unwrap();
        let selection = cv
            .fills
            .iter()
            .find_map(|(rect, color)| (*color == SEL_COLOR).then_some(*rect))
            .expect("必须绘制选区背景");
        assert!((selection.size.height - layout.height()).abs() < 0.01);
        assert!(
            (selection.top() - Edit::layout_y(layout, layout::content_rect(edit.base()))).abs()
                < 0.01
        );
        assert!((edit.text_input_rect().unwrap().size.height - layout.height()).abs() < 0.01);
        assert_eq!(edit.text_input_rect().unwrap().size.width, CARET_WIDTH);
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

    #[test]
    fn 多行不创建整段单行排版缓存() {
        let cv = FakeCanvas;
        let mut edit = Edit::new().multiline(true).text("first\nsecond");

        layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 80.0), &cv);

        assert!(edit.display_layout.is_none());
        assert_eq!(edit.lines.len(), 2);
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
        assert!(e.scroll.get().offset().x > 0.0);
        let content = layout::content_rect(e.base());
        let caret = e.text_input_rect().unwrap();
        assert!(caret.left() >= content.left() && caret.right() <= content.right() + 2.0);
        assert!(e.hit_index(Point::new(content.right() - 1.0, content.top() + 2.0)) >= 8);
    }

    #[test]
    fn 光标移动和删除遵守grapheme边界() {
        let mut combining = Edit::new().text("a\u{301}b");
        combining.on_event(&kd(keys::HOME, false));
        combining.on_event(&kd(keys::RIGHT, false));
        assert_eq!(combining.cursor(), 2, "组合音标必须与基础字符一起移动");
        combining.on_event(&kd(keys::BACKSPACE, false));
        assert_eq!(combining.base().text, "b");

        let family = "👨‍👩‍👧‍👦";
        let mut emoji = Edit::new().text(format!("{family}x"));
        emoji.on_event(&kd(keys::HOME, false));
        emoji.on_event(&kd(keys::RIGHT, false));
        assert_eq!(
            emoji.cursor(),
            family.chars().count(),
            "ZWJ emoji 必须整体移动"
        );
        emoji.on_event(&kd(keys::DELETE, false));
        assert_eq!(emoji.base().text, family, "Delete 只删除下一个 grapheme");
    }

    #[test]
    fn 鼠标命中不会停在grapheme内部() {
        let boundaries = Edit::grapheme_char_boundaries("a\u{301}b");
        assert_eq!(boundaries, vec![0, 2, 3]);
        assert_eq!(
            Edit::snap_to_grapheme_boundary(1, 16.0, &boundaries, |index| index as f32 * 10.0),
            2
        );

        let family = "👨‍👩‍👧‍👦";
        let boundaries = Edit::grapheme_char_boundaries(&format!("{family}x"));
        let family_end = family.chars().count();
        assert_eq!(boundaries, vec![0, family_end, family_end + 1]);
        assert_eq!(
            Edit::snap_to_grapheme_boundary(3, 65.0, &boundaries, |index| index as f32 * 10.0),
            family_end
        );
    }
}
