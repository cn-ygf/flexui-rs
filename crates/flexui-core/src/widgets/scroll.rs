//! ScrollView：纵向滚动容器（内容超出视口可滚；滚轮驱动，绘制滚动条）。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout::{self, measure_node};
use crate::style::StyleSpec;
use crate::widget::{Base, Container, Node, Widget, WidgetRole};

/// 纵向滚动容器。子控件纵向堆叠；超出视口的部分被裁剪，可用滚轮滚动。
pub struct ScrollView {
    base: Base,
}

impl ScrollView {
    pub fn new() -> Self {
        let mut base = Base::new(WidgetRole::Plain);
        base.scrollable = true;
        Self { base }
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
        let mut heights: Vec<f32> = Vec::with_capacity(n);
        let mut total = 0.0f32;
        for child in self.base.children.iter_mut() {
            let s = measure_node(child.as_mut(), inner, cv);
            heights.push(s.height);
            total += s.height;
        }
        if n > 1 {
            total += self.base.spacing * (n as f32 - 1.0);
        }
        self.base.content_h = total;
        // 夹取滚动偏移到 [0, max(0, total - 视口高)]。
        let max_scroll = (total - content.size.height).max(0.0);
        if self.base.scroll_y > max_scroll {
            self.base.scroll_y = max_scroll;
        }
        if self.base.scroll_y < 0.0 {
            self.base.scroll_y = 0.0;
        }
        // 从 content.top - scroll_y 起纵向摆放（超出部分由绘制裁剪）。
        let spacing = self.base.spacing;
        let mut y = content.top() - self.base.scroll_y;
        for (i, child) in self.base.children.iter_mut().enumerate() {
            let rect = Rect::new(content.left(), y, content.size.width, heights[i]);
            layout::layout_node(child.as_mut(), rect, cv);
            y += heights[i] + spacing;
        }
    }

    fn paint_content(&self, cv: &mut dyn Canvas, _style: &StyleSpec) {
        // 内容高于视口时画一条右侧滚动条。
        let view_h = self.base.rect.size.height;
        let content_h = self.base.content_h;
        if content_h <= view_h || view_h <= 0.0 {
            return;
        }
        let r = self.base.rect;
        let bar_w = 5.0;
        let bx = r.right() - bar_w - 2.0;
        // 轨道
        cv.fill_round_rect(
            Rect::new(bx, r.top() + 2.0, bar_w, view_h - 4.0),
            Corners::all(bar_w / 2.0),
            Color::from_u8(255, 255, 255, 30),
        );
        // 滑块
        let ratio = view_h / content_h;
        let thumb_h = (view_h * ratio).max(24.0);
        let scroll_ratio = self.base.scroll_y / (content_h - view_h);
        let thumb_y = r.top() + 2.0 + (view_h - 4.0 - thumb_h) * scroll_ratio;
        cv.fill_round_rect(
            Rect::new(bx, thumb_y, bar_w, thumb_h),
            Corners::all(bar_w / 2.0),
            Color::from_u8(200, 210, 230, 160),
        );
    }
}

common_builders!(ScrollView);

impl Container for ScrollView {}
