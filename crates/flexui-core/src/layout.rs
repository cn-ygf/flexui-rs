//! 布局引擎（L3）：两遍式 measure（求期望尺寸）+ arrange（摆放绝对矩形）。
//!
//! - 默认容器（Panel/Box）：子控件叠放，各自填充内容区（也支撑「单子嵌套」）。
//! - VBox/HBox：主轴顺序排列 + `flex_grow` 分配剩余空间，交叉轴拉伸。
//! - 所有节点的 `rect` 最终是窗口逻辑坐标下的绝对矩形，供绘制与命中测试直接使用。

use flexui_geometry::{Rect, Size};
use flexui_gfx::Canvas;

use crate::widget::{Base, Widget};

/// 主轴方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

/// 内容区 = 自身矩形去掉 padding。
pub fn content_rect(b: &Base) -> Rect {
    b.rect.deflate(b.padding)
}

/// 对一个节点做完整布局：设置其绝对矩形，再摆放其子控件。
pub fn layout_node(node: &mut dyn Widget, rect: Rect, cv: &dyn Canvas) {
    node.base_mut().rect = rect;
    let content = content_rect(node.base());
    node.arrange(content, cv);
}

/// 度量一个节点（不可见的返回 0）。
pub fn measure_node(node: &mut dyn Widget, avail: Size, cv: &dyn Canvas) -> Size {
    if !node.base().visible {
        return Size::default();
    }
    node.measure(avail, cv)
}

/// 默认度量：取子控件最大尺寸并加上 padding；显式尺寸优先。
pub fn measure_stack(b: &mut Base, avail: Size, cv: &dyn Canvas) -> Size {
    let inner = Size::new(
        (avail.width - b.padding.horizontal()).max(0.0),
        (avail.height - b.padding.vertical()).max(0.0),
    );
    let mut w = 0.0f32;
    let mut h = 0.0f32;
    for child in b.children.iter_mut() {
        let s = measure_node(child.as_mut(), inner, cv);
        w = w.max(s.width);
        h = h.max(s.height);
    }
    size_from_content(b, w, h)
}

/// 默认摆放：每个子控件都填充内容区（叠放 / 单子）。
pub fn arrange_stack(b: &mut Base, content: Rect, cv: &dyn Canvas) {
    for child in b.children.iter_mut() {
        layout_node(child.as_mut(), content, cv);
    }
}

/// 沿主轴度量（VBox/HBox 用）：主轴累加 + 间距，交叉轴取最大。
pub fn measure_axis(b: &mut Base, axis: Axis, avail: Size, cv: &dyn Canvas) -> Size {
    let inner = Size::new(
        (avail.width - b.padding.horizontal()).max(0.0),
        (avail.height - b.padding.vertical()).max(0.0),
    );
    let n = b.children.len();
    let mut main = 0.0f32;
    let mut cross = 0.0f32;
    for child in b.children.iter_mut() {
        let s = measure_node(child.as_mut(), inner, cv);
        match axis {
            Axis::Vertical => {
                main += s.height;
                cross = cross.max(s.width);
            }
            Axis::Horizontal => {
                main += s.width;
                cross = cross.max(s.height);
            }
        }
    }
    if n > 1 {
        main += b.spacing * (n as f32 - 1.0);
    }
    let (w, h) = match axis {
        Axis::Vertical => (cross, main),
        Axis::Horizontal => (main, cross),
    };
    size_from_content(b, w, h)
}

/// 沿主轴摆放（VBox/HBox 用）：固定尺寸子控件用其期望值，flex_grow 子控件分摊剩余。
pub fn arrange_axis(b: &mut Base, axis: Axis, content: Rect, cv: &dyn Canvas) {
    let n = b.children.len();
    if n == 0 {
        return;
    }
    let inner = Size::new(content.size.width, content.size.height);

    // 先度量每个子控件的期望主轴尺寸。
    let mut desired: Vec<Size> = Vec::with_capacity(n);
    let mut grow_sum = 0.0f32;
    let mut fixed_main = 0.0f32;
    for child in b.children.iter_mut() {
        let s = measure_node(child.as_mut(), inner, cv);
        let g = child.base().flex_grow;
        grow_sum += g;
        if g <= 0.0 {
            fixed_main += main_of(axis, s);
        }
        desired.push(s);
    }
    let total_spacing = b.spacing * (n as f32 - 1.0);
    let main_avail = main_of(axis, content.size);
    let free = (main_avail - total_spacing - fixed_main).max(0.0);

    // 逐个摆放。
    let spacing = b.spacing;
    let mut cursor = main_start(axis, content);
    for (i, child) in b.children.iter_mut().enumerate() {
        let g = child.base().flex_grow;
        let main_size = if g > 0.0 && grow_sum > 0.0 {
            free * (g / grow_sum)
        } else {
            main_of(axis, desired[i])
        };
        let rect = match axis {
            Axis::Vertical => Rect::new(content.left(), cursor, content.size.width, main_size),
            Axis::Horizontal => Rect::new(cursor, content.top(), main_size, content.size.height),
        };
        layout_node(child.as_mut(), rect, cv);
        cursor += main_size + spacing;
    }
}

fn main_of(axis: Axis, s: Size) -> f32 {
    match axis {
        Axis::Vertical => s.height,
        Axis::Horizontal => s.width,
    }
}

fn main_start(axis: Axis, content: Rect) -> f32 {
    match axis {
        Axis::Vertical => content.top(),
        Axis::Horizontal => content.left(),
    }
}

/// 由「内容尺寸」加上 padding、并应用显式宽高覆盖，得到控件最终尺寸。
/// 叶子控件（Label/Button 等）度量时可直接复用。
pub fn size_from_content(b: &Base, mut w: f32, mut h: f32) -> Size {
    w += b.padding.horizontal();
    h += b.padding.vertical();
    if let Some(x) = b.width {
        w = x;
    }
    if let Some(y) = b.height {
        h = y;
    }
    Size::new(w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{HBox, Panel, VBox};
    use flexui_gfx::Font;

    /// 测试用假画布：文字度量按固定字宽估算，避免依赖平台。
    struct FakeCanvas;
    impl Canvas for FakeCanvas {
        fn fill_rect(&mut self, _r: Rect, _c: flexui_geometry::Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: flexui_geometry::Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: flexui_geometry::Corners, _c: flexui_geometry::Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: flexui_geometry::Corners, _c: flexui_geometry::Color, _w: f32) {}
        fn draw_text(&mut self, _t: &str, _o: flexui_geometry::Point, _f: &Font, _c: flexui_geometry::Color) {}
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
    }

    #[test]
    fn vbox_按期望高度纵向排列() {
        let mut root = VBox::new();
        root.base_mut().children.push(Box::new(Panel::new().size(100.0, 30.0)));
        root.base_mut().children.push(Box::new(Panel::new().size(100.0, 50.0)));
        root.base_mut().spacing = 10.0;
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 400.0), &cv);
        let c = &root.base().children;
        assert_eq!(c[0].base().rect, Rect::new(0.0, 0.0, 200.0, 30.0));
        // 第二个：y = 30 + spacing 10 = 40
        assert_eq!(c[1].base().rect, Rect::new(0.0, 40.0, 200.0, 50.0));
    }

    #[test]
    fn vbox_flex_grow_分摊剩余空间() {
        let mut root = VBox::new();
        // 固定 40 高 + 一个 grow=1 的填充
        root.base_mut().children.push(Box::new(Panel::new().size(10.0, 40.0)));
        let mut fill = Panel::new();
        fill.base_mut().flex_grow = 1.0;
        root.base_mut().children.push(Box::new(fill));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 200.0), &cv);
        let c = &root.base().children;
        assert_eq!(c[0].base().rect.size.height, 40.0);
        // 剩余 200-40 = 160 全给 grow 项
        assert_eq!(c[1].base().rect.size.height, 160.0);
    }

    #[test]
    fn hbox_横向排列() {
        let mut root = HBox::new();
        root.base_mut().children.push(Box::new(Panel::new().size(30.0, 20.0)));
        root.base_mut().children.push(Box::new(Panel::new().size(50.0, 20.0)));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 100.0), &cv);
        let c = &root.base().children;
        assert_eq!(c[0].base().rect.left(), 0.0);
        assert_eq!(c[1].base().rect.left(), 30.0);
    }

    #[test]
    fn padding_收缩内容区() {
        let mut root = Panel::new();
        root.base_mut().padding = flexui_geometry::Insets::all(10.0);
        root.base_mut().children.push(Box::new(Panel::new()));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
        // 子控件填充内容区：(10,10,80,80)
        assert_eq!(root.base().children[0].base().rect, Rect::new(10.0, 10.0, 80.0, 80.0));
    }
}
