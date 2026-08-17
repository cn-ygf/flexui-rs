//! ListView：可滚动列表（自绘行，点击选中，裁剪在自身范围内）。
//!
//! 行、选择与滚动状态均由本控件持有；分发器通过 Widget 的滚动能力转发滚轮，
//! 点击选中经指针转发到 `on_event`，选择变化经 name 上报。

use flexui_geometry::{Color, Point, Rect, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::anim::AnimProp;
use crate::common_builders;
use crate::event::{Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::scroll::{paint_scrollbars, ScrollAxes, ScrollBarStyle, ScrollState};
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole};

/// 选中行高亮色。
const SEL_COLOR: Color = Color::rgba(0.20, 0.45, 0.95, 0.45);

/// 可滚动列表：给定字符串项，自绘每行，点击选中。
pub struct ListView {
    base: Base,
    items: Vec<String>,
    row_h: f32,
    selection: Option<usize>,
    scroll: ScrollState,
    scrollbar: ScrollBarStyle,
}

impl ListView {
    pub fn new() -> Self {
        Self {
            base: Base::new_kind(WidgetRole::ListView, WidgetKind::ListView),
            items: Vec::new(),
            row_h: 28.0,
            selection: None,
            scroll: ScrollState::new(ScrollAxes::vertical()),
            scrollbar: ScrollBarStyle::default(),
        }
    }

    /// 若已知视口，滚动使第 i 行进入可见区。
    fn ensure_row_visible(&mut self, i: usize) {
        let row = Rect::new(0.0, i as f32 * self.row_h, self.scroll.viewport().width, self.row_h);
        self.scroll.ensure_visible(row, 0.0);
    }

    /// 滚动条所用视口：纵向跟随内容区，横向贴控件外缘。
    fn scrollbar_viewport(&self) -> Rect {
        let content = layout::content_rect(&self.base);
        Rect::new(
            content.left(),
            content.top(),
            (self.base.rect.right() - content.left()).max(0.0),
            content.size.height,
        )
    }
    /// 设置列表项。
    pub fn items(mut self, it: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.items = it.into_iter().map(Into::into).collect();
        self
    }
    /// 行高。
    pub fn row_height(mut self, h: f32) -> Self {
        self.row_h = h.max(1.0);
        self
    }
    /// 预选中第 i 行。
    pub fn selected(mut self, i: usize) -> Self {
        if i < self.items.len() {
            self.selection = Some(i);
        }
        self
    }
    /// 当前选中行（未选中返回 None）。
    pub fn selection(&self) -> Option<usize> {
        self.selection
    }
}

impl Default for ListView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ListView {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        layout::size_from_content(&self.base, 160.0, self.items.len() as f32 * self.row_h)
    }
    fn arrange(&mut self, content: Rect, _cv: &dyn Canvas) {
        // 内容总高 = 行数×行高；更新滚动度量并夹取偏移。
        let total = self.items.len() as f32 * self.row_h;
        self.scroll
            .set_metrics(Size::new(content.size.width, total), content.size);
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        if content.size.width <= 0.0 || content.size.height <= 0.0 {
            return;
        }
        let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
        let scroll = self.scroll.offset().y;
        cv.save();
        cv.clip_rect(content);
        for (i, item) in self.items.iter().enumerate() {
            let y = content.top() + i as f32 * self.row_h - scroll;
            if y + self.row_h < content.top() || y > content.bottom() {
                continue; // 不在视口内
            }
            let row = Rect::new(content.left(), y, content.size.width, self.row_h);
            if self.selection == Some(i) {
                cv.fill_rect(row, style.selection_color.unwrap_or(SEL_COLOR));
            }
            let text_rect = Rect::new(
                row.left() + 8.0,
                row.top(),
                (row.size.width - 8.0).max(0.0),
                row.size.height,
            );
            draw_aligned_text(
                cv,
                item,
                text_rect,
                &self.base.font,
                color,
                TextAlign::Left,
                true,
            );
        }
        cv.restore();
        // 内容超出视口时用统一绘制器画滚动条（贴控件外缘）。
        paint_scrollbars(
            cv,
            self.scrollbar_viewport(),
            &self.scroll,
            &self.scrollbar,
            style,
        );
    }
    fn on_event(&mut self, ev: &Event) -> EventFlow {
        if let Event::MouseDown {
            pos,
            button: MouseButton::Left,
        } = ev
        {
            let content = layout::content_rect(&self.base);
            let rel = pos.y - content.top() + self.scroll.offset().y;
            if rel >= 0.0 {
                let row = (rel / self.row_h) as usize;
                if row < self.items.len() {
                    self.selection = Some(row);
                }
            }
            return EventFlow::Consumed;
        }
        EventFlow::Ignored
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::SelectedIndex(index) => self.set_selected_index(index),
            WidgetProperty::Items(items) => {
                self.items = items;
                self.selection = self.selection.filter(|index| *index < self.items.len());
                true
            }
            WidgetProperty::RowHeight(height) => {
                self.row_h = height.max(1.0);
                true
            }
            _ => false,
        }
    }
    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        match key {
            WidgetPropertyKey::Items => Some(WidgetProperty::Items(self.items.clone())),
            WidgetPropertyKey::SelectedIndex => self.selection.map(WidgetProperty::SelectedIndex),
            WidgetPropertyKey::RowHeight => Some(WidgetProperty::RowHeight(self.row_h)),
            _ => None,
        }
    }
    fn selected_index(&self) -> Option<usize> {
        self.selection
    }
    fn set_selected_index(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        let changed = self.selection != Some(index);
        self.selection = Some(index);
        // 选中行滚动到可见（视口已知时才有效）。
        self.ensure_row_visible(index);
        changed
    }
    fn is_scrollable(&self) -> bool {
        true
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.scroll.scroll_by(dx, dy)
    }
    fn scroll_offset(&self) -> Option<Point> {
        Some(self.scroll.offset())
    }
    fn scrollbar_grab(&self, pos: Point) -> Option<crate::scroll::ScrollGrab> {
        crate::scroll::thumb_grab(&self.scroll, self.scrollbar_viewport(), &self.scrollbar, pos)
    }
    fn scrollbar_drag(&mut self, pos: Point, grab: &crate::scroll::ScrollGrab) -> bool {
        let viewport = self.scrollbar_viewport();
        crate::scroll::apply_thumb_drag(&mut self.scroll, viewport, &self.scrollbar, pos, grab)
    }
    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        self.scroll.axis_value(prop)
    }
    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        self.scroll.set_axis_value(prop, value)
    }
}

common_builders!(ListView);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listview_点击选中行() {
        let mut lv = ListView::new().items(["a", "b", "c", "d"]).row_height(20.0);
        // 布局：视口高 40（只显示 2 行），内容高 80。
        lv.base_mut().rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        lv.arrange(Rect::new(0.0, 0.0, 100.0, 40.0), &Fake);
        assert_eq!(lv.scroll.content().height, 80.0);
        // 点击 y=10 → 第 0 行。
        lv.on_event(&Event::MouseDown {
            pos: flexui_geometry::Point::new(10.0, 10.0),
            button: MouseButton::Left,
        });
        assert_eq!(lv.selection(), Some(0));
        // 滚动后点击。
        assert!(lv.scroll_by(0.0, -20.0));
        lv.on_event(&Event::MouseDown {
            pos: flexui_geometry::Point::new(10.0, 10.0),
            button: MouseButton::Left,
        });
        assert_eq!(lv.selection(), Some(1), "滚动 20 后 y=10 对应第 1 行");
    }

    struct Fake;
    impl Canvas for Fake {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: flexui_geometry::Corners, _c: Color) {}
        fn stroke_round_rect(
            &mut self,
            _r: Rect,
            _rad: flexui_geometry::Corners,
            _c: Color,
            _w: f32,
        ) {
        }
        fn draw_text(
            &mut self,
            _t: &str,
            _o: flexui_geometry::Point,
            _f: &flexui_gfx::Font,
            _c: Color,
        ) {
        }
        fn measure_text(&self, t: &str, f: &flexui_gfx::Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
    }
}
