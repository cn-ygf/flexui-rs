//! Label：文本标签控件（不可交互；可选开启文本选中 + Cmd+C 复制）。
//! 设置 `wrap_width` 后按最大宽度自动换行（测量/绘制/命中/选区/复制均按多行处理）。

use std::cell::RefCell;

use flexui_gfx::{Canvas, Font, TextAlign, TextLayout};
use flexui_gfx::{Color, Point, Rect, Size};

use crate::common_builders;
use crate::event::{Event, EventFlow, MouseButton};
use crate::layout;
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

    /// 单行超出内容宽时做尾部省略；宽度放得下则返回 None，直接复用原排版。
    ///
    /// 必须与 `compute_lines` 走同一套排版（`layout_text`），否则在「控件宽度按内容
    /// 自适应」时会误判：布局分配的宽度恰好等于排版宽度，若改用另一套引擎度量，
    /// 只要它算出的宽度大一点点就会截断本来放得下的文本。
    fn elide_layout(
        cv: &dyn Canvas,
        line: &CachedLine,
        font: &Font,
        max_w: f32,
    ) -> Option<TextLayout> {
        if line.width <= max_w {
            return None;
        }
        const ELL: &str = "…";
        let ellipsis = cv.layout_text(ELL, font);
        let budget = max_w - ellipsis.width();
        if budget <= 0.0 {
            return Some(ellipsis);
        }
        // 先用已有排版的字符边界定位可保留的前缀长度，避免逐字重新排版。
        let mut n = line.layout.char_count();
        while n > 0 && line.layout.x_for_char(n) > budget {
            n -= 1;
        }
        // 前缀加省略号后整体重新排版校验；连写宽度略有出入时再退一个字符。
        loop {
            let mut shown: String = line.layout.text().chars().take(n).collect();
            shown.push_str(ELL);
            let candidate = cv.layout_text(&shown, font);
            if n == 0 || candidate.width() <= max_w {
                return Some(candidate);
            }
            n -= 1;
        }
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
            // 换行模式下每行已按预算宽度断好，只有单行模式需要按内容宽做尾部省略。
            let elided = if self.wrap_width.is_some() {
                None
            } else {
                Self::elide_layout(cv, line, &self.base.font, content.size.width)
            };
            let layout = elided.as_ref().unwrap_or(&line.layout);
            // 换行模式按行宽左对齐，单行模式在内容区内按对齐方式摆放。
            let ox = if self.wrap_width.is_some() {
                Self::line_origin_x(content, line.width, align)
            } else {
                Self::line_origin_x(content, layout.width(), align)
            };

            // 选区高亮（与本行相交部分；截断后只高亮实际显示出来的字符）。
            if let Some((lo, hi)) = sel {
                let l0 = lo.max(line.start);
                let l1 = hi.min(line.start + line.len);
                if l1 > l0 {
                    let shown = layout.char_count();
                    let r0 = (l0 - line.start).min(shown);
                    let r1 = (l1 - line.start).min(shown);
                    for r in layout.selection_rects(r0..r1, y, c.line_h) {
                        cv.fill_rect(
                            Rect::new(ox + r.left(), r.top(), r.size.width, r.size.height),
                            sel_color,
                        );
                    }
                }
            }

            // 直接消费排版结果绘制，保证绘制与测量、截断判断三者用的是同一套排版。
            let ty = y + ((c.line_h - layout.height()) * 0.5).max(0.0);
            cv.draw_text_layout(layout, Point::new(ox, ty), color);
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
    use crate::layout::layout_node;
    use crate::sizing::Align;
    use crate::style::StyleSpec;
    use crate::widgets::VBox;
    use flexui_gfx::{Color, Corners, Size};

    /// 模拟 Windows 后端的两套文字引擎：`measure_text` 是 GDI+ 包围盒（偏宽），
    /// `measure_text_advance_size` 是排版前进宽度（`layout_text` 默认实现走它）。
    /// 两者差值即为历史 bug 的触发条件。
    #[derive(Default)]
    struct 双引擎画布 {
        绘制文本: Vec<String>,
    }

    impl 双引擎画布 {
        /// 前进宽度：每字符按 0.5 字号。
        fn 前进宽度(text: &str, font: &Font) -> f32 {
            text.chars().count() as f32 * font.size * 0.5
        }
    }

    impl Canvas for 双引擎画布 {
        fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _c: Corners, _col: Color, _w: f32) {}
        fn draw_text(&mut self, text: &str, _origin: Point, _font: &Font, _color: Color) {
            self.绘制文本.push(text.to_owned());
        }
        /// 包围盒比前进宽度多 4px，模拟 GDI+ 的 overhang 余量。
        fn measure_text(&self, text: &str, font: &Font) -> Size {
            Size::new(Self::前进宽度(text, font) + 4.0, font.size * 1.2)
        }
        fn measure_text_advance_size(&self, text: &str, font: &Font) -> Size {
            Size::new(Self::前进宽度(text, font), font.size * 1.2)
        }
    }

    /// 回归：控件宽度按内容自适应时，布局分配的宽度恰好等于排版宽度，
    /// 绘制不得因为改用另一套引擎度量而误加省略号。
    #[test]
    fn 内容自适应宽度不应误加省略号() {
        let mut cv = 双引擎画布::default();
        // 交叉轴用 Start，子控件才按自身内容宽度收敛；Stretch 会拉满而测不出问题。
        let mut root = VBox::new()
            .align(Align::Start)
            .push(Label::new("未连接").font_size(24.0));
        layout_node(&mut root, Rect::new(0.0, 0.0, 800.0, 100.0), &cv);

        let label = &root.base().children[0];
        assert_eq!(
            label.base().rect.size.width,
            双引擎画布::前进宽度("未连接", &label.base().font),
            "内容自适应宽度应等于排版前进宽度"
        );

        label.paint_content(&mut cv, &StyleSpec::default());
        assert_eq!(
            cv.绘制文本,
            vec!["未连接".to_owned()],
            "宽度刚好放得下时不应截断"
        );
    }

    /// 宽度确实不够时仍要正常截断，且截断结果必须放得进内容区。
    #[test]
    fn 宽度不足时按排版宽度截断() {
        let mut cv = 双引擎画布::default();
        let mut root = VBox::new().push(Label::new("一二三四五六").font_size(20.0).width(40.0));
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 60.0), &cv);

        let label = &root.base().children[0];
        assert_eq!(
            label.base().rect.size.width,
            40.0,
            "固定宽度应优先于父级拉伸"
        );

        label.paint_content(&mut cv, &StyleSpec::default());
        let shown = &cv.绘制文本[0];
        assert!(shown.ends_with('…'), "应以省略号结尾：{shown}");
        assert!(
            双引擎画布::前进宽度(shown, &label.base().font) <= 40.0,
            "截断结果应放得进内容区：{shown}"
        );
    }

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
