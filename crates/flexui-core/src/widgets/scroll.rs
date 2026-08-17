//! ScrollView：纵向滚动容器（内容超出视口可滚；滚轮驱动，绘制滚动条）。
//!
//! 滚动状态复用统一的 `ScrollState`（见 `crate::scroll`），与 ListView/Edit 共用一套逻辑。

use flexui_geometry::{Point, Rect, Size};
use flexui_gfx::Canvas;

use crate::anim::AnimProp;
use crate::common_builders;
use crate::layout::{self, measure_node};
use crate::scroll::{paint_scrollbars, ScrollAxes, ScrollState};
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

// 滚动条外观类型统一放在 crate::scroll，这里再导出以保持旧路径。
pub use crate::scroll::ScrollBarStyle;

/// 纵向滚动容器。子控件纵向堆叠；超出视口的部分被裁剪，可用滚轮滚动。
pub struct ScrollView {
    base: Base,
    scroll: ScrollState,
    scrollbar: ScrollBarStyle,
}

impl ScrollView {
    pub fn new() -> Self {
        Self {
            base: Base::new_kind(WidgetRole::Plain, WidgetKind::ScrollView),
            scroll: ScrollState::new(ScrollAxes::vertical()),
            scrollbar: ScrollBarStyle::default(),
        }
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.base.spacing = s;
        self
    }
    pub fn push(mut self, child: impl Widget + 'static) -> Self {
        self.base.children.push(Box::new(child));
        self
    }
    pub fn push_node(mut self, child: Node) -> Self {
        self.base.children.push(child);
        self
    }
    pub fn scrollbar_style(mut self, style: ScrollBarStyle) -> Self {
        self.scrollbar = style;
        self
    }

    fn viewport(&self) -> Rect {
        layout::content_rect(&self.base)
    }

    /// 子控件使用的视口始终避开滚动条，避免内容宽度随滚动状态跳动。
    fn content_viewport(&self) -> Rect {
        let viewport = self.viewport();
        let reserved =
            (self.scrollbar.width + self.scrollbar.gap + self.scrollbar.margin).max(0.0);
        Rect::new(
            viewport.left(),
            viewport.top(),
            (viewport.size.width - reserved).max(0.0),
            viewport.size.height,
        )
    }

    fn clip_viewport(&self) -> Rect {
        let content = self.content_viewport();
        let left = (content.left() - 1.0).max(self.base.rect.left());
        let right = (content.right() + 1.0).min(self.viewport().right());
        Rect::new(left, content.top(), right - left, content.size.height)
    }

    #[cfg(test)]
    fn max_scroll(&self) -> f32 {
        self.scroll.max().y
    }

    /// 按滚动增量把已布局的子树整体平移，使滚动立即生效（无需重新布局）。
    fn shift_children(&mut self, before: Point) {
        let after = self.scroll.offset();
        let dx = before.x - after.x;
        let dy = before.y - after.y;
        if dx != 0.0 || dy != 0.0 {
            for child in &mut self.base.children {
                translate_subtree(child.as_mut(), dx, dy);
            }
        }
    }
}

impl Default for ScrollView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ScrollView {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn measure(&mut self, avail: Size, _cv: &dyn Canvas) -> Size {
        // 视口尺寸：显式尺寸优先，否则用可用空间（滚动容器通常 Fill/Fixed）。
        layout::size_from_content(&self.base, avail.width, avail.height)
    }

    fn arrange(&mut self, _content: Rect, cv: &dyn Canvas) {
        let content = self.content_viewport();
        let inner = Size::new(content.size.width, content.size.height);
        // 先量各子控件高度，求内容总高。
        let n = self.base.children.len();
        let mut heights = vec![0.0f32; n];
        let mut total = 0.0f32;
        let mut visible_count = 0usize;
        for (i, child) in self.base.children.iter_mut().enumerate() {
            if !child.base().visible {
                continue;
            }
            let s = measure_node(child.as_mut(), inner, cv);
            heights[i] = s.height;
            total += s.height;
            visible_count += 1;
        }
        if visible_count > 1 {
            total += self.base.spacing * (visible_count as f32 - 1.0);
        }
        // 更新滚动度量并夹取偏移（内容高 total、视口 = 子内容视口）。
        self.scroll
            .set_metrics(Size::new(content.size.width, total), content.size);
        // 从 content.top - offset.y 起纵向摆放（超出部分由绘制裁剪）。
        let spacing = self.base.spacing;
        let mut y = content.top() - self.scroll.offset().y;
        for (i, child) in self.base.children.iter_mut().enumerate() {
            if !child.base().visible {
                continue;
            }
            let rect = Rect::new(content.left(), y, content.size.width, heights[i]);
            layout::layout_node(child.as_mut(), rect, cv);
            y += heights[i] + spacing;
        }
    }

    fn children_viewport(&self) -> Rect {
        self.clip_viewport()
    }

    fn paint_foreground(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        // 内容高于视口时在子内容之上绘制滚动条（统一绘制器）。
        paint_scrollbars(cv, self.viewport(), &self.scroll, &self.scrollbar, style);
    }
    fn is_scrollable(&self) -> bool {
        true
    }
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let before = self.scroll.offset();
        let changed = self.scroll.scroll_by(dx, dy);
        if changed {
            self.shift_children(before);
        }
        changed
    }
    fn scroll_offset(&self) -> Option<Point> {
        Some(self.scroll.offset())
    }
    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        self.scroll.axis_value(prop)
    }
    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        let before = self.scroll.offset();
        let handled = self.scroll.set_axis_value(prop, value);
        if handled {
            self.shift_children(before);
        }
        handled
    }
}

common_builders!(ScrollView);

impl Container for ScrollView {}

fn translate_subtree(node: &mut dyn Widget, dx: f32, dy: f32) {
    node.base_mut().rect.origin.x += dx;
    node.base_mut().rect.origin.y += dy;
    let count = node.base().children.len();
    for index in 0..count {
        translate_subtree(node.base_mut().children[index].as_mut(), dx, dy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_node;
    use crate::widgets::Panel;
    use flexui_geometry::{Color, Corners};
    use flexui_gfx::Font;

    struct FakeCanvas;
    impl Canvas for FakeCanvas {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, _t: &str, _o: flexui_geometry::Point, _f: &Font, _c: Color) {}
        fn measure_text(&self, text: &str, font: &Font) -> Size {
            Size::new(text.len() as f32 * font.size * 0.5, font.size)
        }
    }

    #[test]
    fn padding决定滚动视口且滚动后立即移动子树() {
        let mut view = ScrollView::new()
            .padding_ltrb(24.0, 16.0, 20.0, 24.0)
            .push(Panel::new().height(120.0))
            .push(Panel::new().height(120.0));
        layout_node(&mut view, Rect::new(0.0, 0.0, 294.0, 228.0), &FakeCanvas);
        // 预留 = 条宽5 + 间距4 + 边距2 = 11，视口宽 250 → 子内容宽 239。
        assert_eq!(
            view.children_viewport(),
            Rect::new(23.0, 16.0, 241.0, 188.0)
        );
        assert_eq!(view.base.children[0].base().rect.left(), 24.0);
        assert_eq!(view.base.children[0].base().rect.size.width, 239.0);
        assert_eq!(view.max_scroll(), 52.0);
        let before = view.base.children[0].base().rect.top();
        assert!(view.scroll_by(0.0, -32.0));
        assert_eq!(view.base.children[0].base().rect.top(), before - 32.0);
        assert_eq!(view.scroll_offset(), Some(Point::new(0.0, 32.0)));
        assert!(!view.scroll_by(0.0, 0.0));
    }
}
