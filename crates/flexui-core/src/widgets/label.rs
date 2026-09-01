//! Label：文本标签控件（不可交互；可选开启文本选中 + Cmd+C 复制）。
//! 设置 `wrap_width` 后按最大宽度自动换行（测量/绘制/命中/选区/复制均按多行处理）。

use std::cell::RefCell;

use flexui_gfx::{Canvas, Font, TextAlign, TextLayout};
use flexui_gfx::{Color, Point, Rect, Size};

use crate::common_builders;
use crate::event::{Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, TextControl, Widget, WidgetProperty, WidgetRole};

/// 一行的整形结果（多行换行后每行一份）。
struct CachedLine {
    layout: TextLayout,
    /// 该行首字符在原始文本中的字符下标。
    start: usize,
    /// 该行字符数。
    len: usize,
    /// 该行整形宽度（像素）。
    width: f32,
}

/// 换行/整形缓存：文本或字体或换行宽度变化时重算。
struct WrapCache {
    text: String,
    wrap_width: f32,
    font_key: (u32, bool, bool, Option<String>),
    lines: Vec<CachedLine>,
    line_h: f32,
    total: Size,
}

fn font_key(font: &Font) -> (u32, bool, bool, Option<String>) {
    (
        font.size.to_bits(),
        font.bold,
        font.italic,
        font.family.clone(),
    )
}

/// 文本标签（不可交互；`selectable` 开启后支持拖选与复制；`wrap_width` 开启自动换行）。
pub struct Label {
    base: Base,
    /// 选区锚点（拖动起点）。
    sel_anchor: Option<usize>,
    /// 光标（拖动当前位置）。
    caret: usize,
    /// 自动换行最大宽度（像素）；None 表示单行。
    wrap_width: Option<f32>,
    cache: RefCell<Option<WrapCache>>,
    /// 绘制时记录内容区左上角，供无 Canvas 的鼠标命中换算行列。
    paint_origin: std::cell::Cell<Point>,
    paint_align: std::cell::Cell<u8>,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new_kind(WidgetRole::Plain, WidgetKind::Label);
        base.text = text.into();
        Self {
            base,
            sel_anchor: None,
            caret: 0,
            wrap_width: None,
            cache: RefCell::new(None),
            paint_origin: std::cell::Cell::new(Point::new(0.0, 0.0)),
            paint_align: std::cell::Cell::new(0),
        }
    }

    /// 设置自动换行最大宽度（像素）。
    pub fn wrap_width(mut self, w: f32) -> Self {
        self.wrap_width = Some(w);
        self.cache.replace(None);
        self
    }

    /// 当前选区（字符区间），无选区返回 None。
    fn sel_range(&self) -> Option<(usize, usize)> {
        let anchor = self.sel_anchor?;
        if anchor == self.caret {
            None
        } else {
            Some((anchor.min(self.caret), anchor.max(self.caret)))
        }
    }

    /// 保证换行缓存与当前文本/字体/换行宽度一致（惰性重算）。
    fn ensure_cache(&self, cv: &dyn Canvas) {
        let wrap = self.wrap_width.unwrap_or(f32::INFINITY);
        let fk = font_key(&self.base.font);
        {
            let cache = self.cache.borrow();
            if let Some(c) = cache.as_ref() {
                if c.text == self.base.text && c.wrap_width == wrap && c.font_key == fk {
                    return;
                }
            }
        }
        let (lines, line_h, total) = self.compute_lines(cv, wrap);
        self.cache.replace(Some(WrapCache {
            text: self.base.text.clone(),
            wrap_width: wrap,
            font_key: fk,
            lines,
            line_h,
            total,
        }));
    }

    /// 按最大宽度贪心断行：优先在空格处断（英文单词不拆），CJK 逐字断；保留显式换行。
    fn compute_lines(&self, cv: &dyn Canvas, wrap: f32) -> (Vec<CachedLine>, f32, Size) {
        let font = &self.base.font;
        let chars: Vec<char> = self.base.text.chars().collect();
        let n = chars.len();
        let full = cv.layout_text(&self.base.text, font);
        let line_h = full.height().max(font.size);

        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut start = 0usize;
        let mut i = 0usize;
        let mut last_space: Option<usize> = None; // 当前行内最后一个空格下标
        while i < n {
            let c = chars[i];
            if c == '\n' {
                ranges.push((start, i - start));
                i += 1;
                start = i;
                last_space = None;
                continue;
            }
            if c == ' ' {
                last_space = Some(i);
            }
            let w = full.x_for_char(i + 1) - full.x_for_char(start);
            if wrap.is_finite() && w > wrap && i > start {
                // 断点：能回退到空格后就不拆词，否则在当前字符硬断。
                let brk = match last_space {
                    Some(s) if s >= start => s + 1,
                    _ => i,
                };
                ranges.push((start, brk - start));
                start = brk;
                last_space = None;
                continue; // i 不前进，在新行内重新评估
            }
            i += 1;
        }
        if start < n || ranges.is_empty() {
            ranges.push((start, n - start));
        }

        let mut lines = Vec::with_capacity(ranges.len());
        let mut max_w = 0.0f32;
        for (s, len) in ranges {
            let text: String = chars[s..s + len].iter().collect();
            let layout = cv.layout_text(&text, font);
            let width = layout.width();
            max_w = max_w.max(width);
            lines.push(CachedLine {
                layout,
                start: s,
                len,
                width,
            });
        }
        let total = Size::new(max_w, line_h * lines.len().max(1) as f32);
        (lines, line_h, total)
    }

    /// 某行相对内容区左缘的绘制起点 x（按对齐）。
    fn line_origin_x(content: Rect, width: f32, align: TextAlign) -> f32 {
        match align {
            TextAlign::Center => content.left() + ((content.size.width - width) * 0.5).max(0.0),
            TextAlign::Right => content.right() - width,
            _ => content.left(),
        }
    }

    /// 坐标点映射到原始文本字符下标（跨行）。
    fn hit_index(&self, pos: Point) -> usize {
        let cache = self.cache.borrow();
        let Some(c) = cache.as_ref() else { return 0 };
        if c.lines.is_empty() {
            return 0;
        }
        let origin = self.paint_origin.get();
        let align = match self.paint_align.get() {
            1 => TextAlign::Center,
            2 => TextAlign::Right,
            _ => TextAlign::Left,
        };
        let content = layout::content_rect(&self.base);
        let rel_y = pos.y - origin.y;
        let mut li = (rel_y / c.line_h).floor() as isize;
        li = li.clamp(0, c.lines.len() as isize - 1);
        let line = &c.lines[li as usize];
        let ox = Self::line_origin_x(content, line.width, align);
        let local = line.layout.closest_char_for_x(pos.x - ox);
        line.start + local.min(line.len)
    }
}

impl Widget for Label {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        // 文本/字体变化由 ensure_cache 按内容比对检测，无需在此作废缓存。
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        self.ensure_cache(cv);
        let (w, h) = self
            .cache
            .borrow()
            .as_ref()
            .map(|c| (c.total.width, c.total.height))
            .unwrap_or((0.0, 0.0));
        layout::size_from_content(&self.base, w, h)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let color = style.fg_color.unwrap_or(Color::BLACK);
        let align = style.text_align.unwrap_or(TextAlign::Left);
        let content = layout::content_rect(&self.base);
        self.ensure_cache(cv);
        let cache = self.cache.borrow();
        let Some(c) = cache.as_ref() else { return };

        // 整块文本纵向居中（单行时与旧行为一致）。
        let block_h = c.line_h * c.lines.len().max(1) as f32;
        let top = content.top() + ((content.size.height - block_h) * 0.5).max(0.0);
        self.paint_origin.set(Point::new(content.left(), top));
        self.paint_align.set(match align {
            TextAlign::Center => 1,
            TextAlign::Right => 2,
            _ => 0,
        });

        let sel = self.base.focused.then(|| self.sel_range()).flatten();
        let sel_color = style
            .selection_color
            .unwrap_or(Color::rgba(0.20, 0.52, 1.0, 0.30));

        for (idx, line) in c.lines.iter().enumerate() {
            let y = top + idx as f32 * c.line_h;
            let ox = Self::line_origin_x(content, line.width, align);
            // 选区高亮（与本行相交部分）。
            if let Some((lo, hi)) = sel {
                let l0 = lo.max(line.start);
                let l1 = hi.min(line.start + line.len);
                if l1 > l0 {
                    for r in line.layout.selection_rects(
                        (l0 - line.start)..(l1 - line.start),
                        y,
                        c.line_h,
                    ) {
                        cv.fill_rect(
                            Rect::new(ox + r.left(), r.top(), r.size.width, r.size.height),
                            sel_color,
                        );
                    }
                }
            }
            // 换行模式：逐行按预算宽度左对齐绘制；单行模式：铺满内容区并按对齐 + 越界省略（沿用旧行为）。
            if self.wrap_width.is_some() {
                let line_rect = Rect::new(ox, y, line.width.max(1.0), c.line_h);
                draw_aligned_text(
                    cv,
                    line.layout.text(),
                    line_rect,
                    &self.base.font,
                    color,
                    TextAlign::Left,
                    true,
                );
            } else {
                let line_rect = Rect::new(content.left(), y, content.size.width, c.line_h);
                draw_aligned_text(
                    cv,
                    line.layout.text(),
                    line_rect,
                    &self.base.font,
                    color,
                    align,
                    true,
                );
            }
        }
    }

    fn on_event(&mut self, ev: &Event) -> EventFlow {
        if !self.base.selectable {
            return EventFlow::Ignored;
        }
        match ev {
            // 按下：光标与锚点都落在点击处（拖动即从此起选）。
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let idx = self.hit_index(*pos);
                self.caret = idx;
                self.sel_anchor = Some(idx);
                EventFlow::Consumed
            }
            // 拖动（分发器仅在按住本控件时转发）：延伸选区。
            Event::MouseMove { pos } => {
                self.caret = self.hit_index(*pos);
                EventFlow::Consumed
            }
            // 双击：整段选中。
            Event::DoubleClick { .. } => {
                self.sel_anchor = Some(0);
                self.caret = self.base.text.chars().count();
                EventFlow::Consumed
            }
            _ => EventFlow::Ignored,
        }
    }

    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::WrapWidth(v) => {
                self.wrap_width = v;
                self.cache.replace(None);
                true
            }
            _ => false,
        }
    }

    fn selected_text(&self) -> Option<String> {
        let (lo, hi) = self.sel_range()?;
        Some(self.base.text.chars().skip(lo).take(hi - lo).collect())
    }
}

common_builders!(Label);

impl TextControl for Label {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 选中子串按字符切片() {
        let mut label = Label::new("你好世界");
        label.sel_anchor = Some(1);
        label.caret = 3;
        assert_eq!(label.selected_text().as_deref(), Some("好世"));
        // 反向拖选结果一致。
        label.sel_anchor = Some(3);
        label.caret = 1;
        assert_eq!(label.selected_text().as_deref(), Some("好世"));
        // 空选区返回 None。
        label.sel_anchor = Some(2);
        label.caret = 2;
        assert_eq!(label.selected_text(), None);
    }

    #[test]
    fn 未开启选中不接收鼠标事件() {
        let mut label = Label::new("abc");
        let flow = label.on_event(&Event::MouseMove {
            pos: Point::new(0.0, 0.0),
        });
        assert_eq!(flow, EventFlow::Ignored);
    }
}
