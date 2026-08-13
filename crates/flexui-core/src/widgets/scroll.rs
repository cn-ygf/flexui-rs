//! ScrollView：纵向滚动容器（内容超出视口可滚；滚轮驱动，绘制滚动条）。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::{Canvas, ImageFit, ImageSource};

use crate::common_builders;
use crate::anim::AnimProp;
use crate::layout::{self, measure_node};
use crate::style::StyleSpec;
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

/// 纵向滚动容器。子控件纵向堆叠；超出视口的部分被裁剪，可用滚轮滚动。
pub struct ScrollView {
    base: Base,
    scroll_y: f32,
    content_h: f32,
    scrollbar: ScrollBarStyle,
}

/// 纵向滚动条外观。
#[derive(Debug, Clone)]
pub struct ScrollBarStyle {
    pub width: f32,
    pub min_thumb_height: f32,
    pub thumb_color: Color,
    pub thumb_image: Option<ImageSource>,
    pub thumb_fit: ImageFit,
}

impl Default for ScrollBarStyle {
    fn default() -> Self {
        Self {
            width: 5.0,
            min_thumb_height: 24.0,
            thumb_color: Color::from_u8(200, 210, 230, 160),
            thumb_image: None,
            thumb_fit: ImageFit::Stretch,
        }
    }
}

impl ScrollView {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Plain),
            scroll_y: 0.0,
            content_h: 0.0,
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

    fn max_scroll(&self) -> f32 {
        (self.content_h - self.viewport().size.height).max(0.0)
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

    fn arrange(&mut self, content: Rect, cv: &dyn Canvas) {
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
        self.content_h = total;
        // 夹取滚动偏移到 [0, max(0, total - 视口高)]。
        let max_scroll = (total - content.size.height).max(0.0);
        if self.scroll_y > max_scroll {
            self.scroll_y = max_scroll;
        }
        if self.scroll_y < 0.0 {
            self.scroll_y = 0.0;
        }
        // 从 content.top - scroll_y 起纵向摆放（超出部分由绘制裁剪）。
        let spacing = self.base.spacing;
        let mut y = content.top() - self.scroll_y;
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
        self.viewport()
    }

    fn paint_foreground(&self, cv: &mut dyn Canvas, _style: &StyleSpec) {
        // 内容高于视口时在子内容之上绘制滚动条。
        let r = self.viewport();
        let view_h = r.size.height;
        let content_h = self.content_h;
        if content_h <= view_h || view_h <= 0.0 {
            return;
        }
        let bar_w = self.scrollbar.width;
        let bx = r.right() - bar_w;
        // 滑块
        let ratio = view_h / content_h;
        let thumb_h = (view_h * ratio).max(self.scrollbar.min_thumb_height).min(view_h);
        let scroll_ratio = self.scroll_y / (content_h - view_h);
        let thumb_y = r.top() + (view_h - thumb_h) * scroll_ratio;
        let thumb = Rect::new(bx, thumb_y, bar_w, thumb_h);
        if let Some(image) = &self.scrollbar.thumb_image {
            cv.draw_image(image, thumb, None, self.scrollbar.thumb_fit.clone());
        } else {
            cv.fill_round_rect(thumb, Corners::all(bar_w / 2.0), self.scrollbar.thumb_color);
        }
    }
    fn is_scrollable(&self) -> bool { true }
    fn scroll_by(&mut self, dy: f32) -> bool {
        let max = self.max_scroll();
        let next = (self.scroll_y - dy).clamp(0.0, max);
        let changed = next != self.scroll_y;
        let applied = self.scroll_y - next;
        self.scroll_y = next;
        if changed {
            for child in &mut self.base.children {
                translate_subtree(child.as_mut(), applied);
            }
        }
        changed
    }
    fn scroll_position(&self) -> Option<f32> { Some(self.scroll_y) }
    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        (prop == AnimProp::ScrollY).then_some(self.scroll_y)
    }
    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        if prop != AnimProp::ScrollY { return false; }
        let max = self.max_scroll();
        let next = value.clamp(0.0, max);
        let applied = self.scroll_y - next;
        self.scroll_y = next;
        if applied != 0.0 {
            for child in &mut self.base.children {
                translate_subtree(child.as_mut(), applied);
            }
        }
        true
    }
}

common_builders!(ScrollView);

impl Container for ScrollView {}

fn translate_subtree(node: &mut dyn Widget, dy: f32) {
    node.base_mut().rect.origin.y += dy;
    let count = node.base().children.len();
    for index in 0..count {
        translate_subtree(node.base_mut().children[index].as_mut(), dy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_node;
    use crate::widgets::Panel;
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
        assert_eq!(view.children_viewport(), Rect::new(24.0, 16.0, 250.0, 188.0));
        assert_eq!(view.max_scroll(), 52.0);
        let before = view.base.children[0].base().rect.top();
        assert!(view.scroll_by(-32.0));
        assert_eq!(view.base.children[0].base().rect.top(), before - 32.0);
        assert_eq!(view.scroll_position(), Some(32.0));
        assert!(!view.scroll_by(0.0));
    }
}
