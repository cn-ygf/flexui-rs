//! Label：文本标签控件（不可交互；可选开启文本选中 + Cmd+C 复制）。

use std::cell::RefCell;

use flexui_gfx::{Canvas, TextAlign, TextLayout};
use flexui_gfx::{Color, Point, Rect, Size};

use crate::common_builders;
use crate::event::{Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, TextControl, Widget, WidgetRole};

/// 命中缓存：绘制时用 Canvas 整形出的布局与文本起点，供无 Canvas 的鼠标命中复用。
struct HitCache {
    layout: TextLayout,
    origin_x: f32,
}

/// 文本标签（不可交互；`selectable` 开启后支持拖选与复制）。
pub struct Label {
    base: Base,
    /// 选区锚点（拖动起点）。
    sel_anchor: Option<usize>,
    /// 光标（拖动当前位置）。
    caret: usize,
    hit_cache: RefCell<Option<HitCache>>,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new_kind(WidgetRole::Plain, WidgetKind::Label);
        base.text = text.into();
        Self { base, sel_anchor: None, caret: 0, hit_cache: RefCell::new(None) }
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

    /// 用绘制时缓存的布局把坐标点映射到字符索引。
    fn hit_index(&self, pos: Point) -> usize {
        match &*self.hit_cache.borrow() {
            Some(cache) => cache.layout.closest_char_for_x(pos.x - cache.origin_x),
            None => 0,
        }
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
        let content = layout::content_rect(&self.base);

        if self.base.selectable {
            // 整形一次，得到字符边界（命中/高亮共用）。
            let text_layout = cv.layout_text(&self.base.text, &self.base.font);
            let text_w = text_layout.width();
            let line_h = text_layout.height().max(self.base.font.size);
            let origin_x = match align {
                TextAlign::Center => content.left() + ((content.size.width - text_w) * 0.5).max(0.0),
                TextAlign::Right => content.right() - text_w,
                _ => content.left(),
            };
            let y = content.top() + ((content.size.height - line_h) * 0.5).max(0.0);

            // 高亮（仅在获得焦点且有选区时）。
            if self.base.focused {
                if let Some((lo, hi)) = self.sel_range() {
                    let sel = style.selection_color.unwrap_or(Color::rgba(0.20, 0.52, 1.0, 0.30));
                    for r in text_layout.selection_rects(lo..hi, y, line_h) {
                        cv.fill_rect(
                            Rect::new(origin_x + r.left(), r.top(), r.size.width, r.size.height),
                            sel,
                        );
                    }
                }
            }
            draw_aligned_text(cv, &self.base.text, content, &self.base.font, color, align, true);
            *self.hit_cache.borrow_mut() = Some(HitCache { layout: text_layout, origin_x });
        } else {
            draw_aligned_text(cv, &self.base.text, content, &self.base.font, color, align, true);
        }
    }

    fn on_event(&mut self, ev: &Event) -> EventFlow {
        if !self.base.selectable {
            return EventFlow::Ignored;
        }
        match ev {
            // 按下：光标与锚点都落在点击处（拖动即从此起选）。
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
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
        let flow = label.on_event(&Event::MouseMove { pos: Point::new(0.0, 0.0) });
        assert_eq!(flow, EventFlow::Ignored);
    }
}
