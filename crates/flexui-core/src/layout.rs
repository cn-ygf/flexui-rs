//! 布局引擎（L3）：两遍式 measure（求期望尺寸）+ arrange（摆放绝对矩形）。
//!
//! - 默认容器（Panel/Box）：子控件叠放，各自填充内容区（也支撑「单子嵌套」）。
//! - VBox/HBox：主轴顺序排列 + `flex_grow` 分配剩余空间，交叉轴拉伸。
//! - 所有节点的 `rect` 最终是窗口逻辑坐标下的绝对矩形，供绘制与命中测试直接使用。

use flexui_geometry::{Insets, Rect, Size};
use flexui_gfx::Canvas;

use crate::sizing::{Align, Justify, Sizing};
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

/// 默认度量：取子控件最大尺寸（含 margin）并加上 padding；显式尺寸优先。
pub fn measure_stack(b: &mut Base, avail: Size, cv: &dyn Canvas) -> Size {
    let inner = Size::new(
        (avail.width - b.padding.horizontal()).max(0.0),
        (avail.height - b.padding.vertical()).max(0.0),
    );
    let mut w = 0.0f32;
    let mut h = 0.0f32;
    for child in b.children.iter_mut() {
        let m = child.base().margin;
        let s = measure_node(child.as_mut(), inner, cv);
        w = w.max(s.width + m.horizontal());
        h = h.max(s.height + m.vertical());
    }
    size_from_content(b, w, h)
}

/// 默认摆放：子控件填充内容区（叠放 / 单子）；设了 `pos` 的子控件绝对定位。
pub fn arrange_stack(b: &mut Base, content: Rect, cv: &dyn Canvas) {
    for child in b.children.iter_mut() {
        let pos = child.base().pos;
        if let Some((px, py)) = pos {
            // 绝对定位：按子控件期望尺寸放到 content.origin + (px,py)。
            let s = measure_node(child.as_mut(), content.size, cv);
            let rect = Rect::new(content.left() + px, content.top() + py, s.width, s.height);
            layout_node(child.as_mut(), rect, cv);
        } else {
            let m = child.base().margin;
            layout_node(child.as_mut(), content.deflate(m), cv);
        }
    }
}

/// 沿主轴度量（VBox/HBox 用）：主轴累加（含 margin）+ 间距，交叉轴取最大。
pub fn measure_axis(b: &mut Base, axis: Axis, avail: Size, cv: &dyn Canvas) -> Size {
    let inner = Size::new(
        (avail.width - b.padding.horizontal()).max(0.0),
        (avail.height - b.padding.vertical()).max(0.0),
    );
    let n = b.children.len();
    let mut main = 0.0f32;
    let mut cross = 0.0f32;
    for child in b.children.iter_mut() {
        let m = child.base().margin;
        let s = measure_node(child.as_mut(), inner, cv);
        main += main_of(axis, s) + main_margin(axis, m);
        cross = cross.max(cross_of(axis, s) + cross_margin(axis, m));
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

/// 沿主轴摆放（VBox/HBox）：Sizing::Fill 子控件分摊剩余（权重 flex_grow，0 视为 1）；
/// 无 Fill 时按 justify 分布剩余空间；交叉轴按 align 对齐；尊重每子 margin。
pub fn arrange_axis(b: &mut Base, axis: Axis, content: Rect, cv: &dyn Canvas) {
    let n = b.children.len();
    if n == 0 {
        return;
    }
    let inner = Size::new(content.size.width, content.size.height);
    let justify = b.justify;
    let align = b.align;
    let spacing = b.spacing;

    // 度量 + 归类（是否 Fill 及其权重）。
    let mut desired: Vec<Size> = Vec::with_capacity(n);
    let mut weights: Vec<f32> = Vec::with_capacity(n);
    let mut grow_sum = 0.0f32;
    let mut fixed_main = 0.0f32;
    for child in b.children.iter_mut() {
        let cb = child.base();
        let m = cb.margin;
        // 主轴 Fill 或 flex_grow>0 都参与撑开（后者兼容旧 flex 用法）。
        let fill = main_sizing(axis, cb).is_fill() || cb.flex_grow > 0.0;
        let weight = if fill {
            if cb.flex_grow > 0.0 {
                cb.flex_grow
            } else {
                1.0
            }
        } else {
            0.0
        };
        let s = measure_node(child.as_mut(), inner, cv);
        fixed_main += main_margin(axis, m);
        if weight > 0.0 {
            grow_sum += weight;
        } else {
            fixed_main += main_of(axis, s);
        }
        desired.push(s);
        weights.push(weight);
    }
    let total_spacing = spacing * (n as f32 - 1.0);
    let main_avail = main_of_size(axis, content.size);
    let free = (main_avail - total_spacing - fixed_main).max(0.0);
    let has_grow = grow_sum > 0.0;

    // 无 Fill 时用 justify 分布剩余空间。
    let (mut cursor, gap_extra) = if has_grow {
        (main_start(axis, content), 0.0)
    } else {
        match justify {
            Justify::Start => (main_start(axis, content), 0.0),
            Justify::Center => (main_start(axis, content) + free / 2.0, 0.0),
            Justify::End => (main_start(axis, content) + free, 0.0),
            Justify::SpaceBetween => {
                let g = if n > 1 { free / (n as f32 - 1.0) } else { 0.0 };
                (main_start(axis, content), g)
            }
            Justify::SpaceAround => {
                let g = free / n as f32;
                (main_start(axis, content) + g / 2.0, g)
            }
        }
    };

    for (i, child) in b.children.iter_mut().enumerate() {
        let m = child.base().margin;
        let main_size = if weights[i] > 0.0 {
            free * (weights[i] / grow_sum)
        } else {
            main_of(axis, desired[i])
        };
        // 交叉轴尺寸与偏移（Stretch 填满，其余按 align 对齐）。
        let cross_avail = (cross_of_size(axis, content.size) - cross_margin(axis, m)).max(0.0);
        let cross_fill = align == Align::Stretch || cross_sizing(axis, child.base()).is_fill();
        let (cross_size, cross_off) = if cross_fill {
            (cross_avail, 0.0)
        } else {
            let cs = cross_of(axis, desired[i]).min(cross_avail);
            let off = match align {
                Align::Start | Align::Stretch => 0.0,
                Align::Center => (cross_avail - cs) / 2.0,
                Align::End => cross_avail - cs,
            };
            (cs, off)
        };

        cursor += main_lead(axis, m);
        let rect = match axis {
            Axis::Vertical => Rect::new(
                content.left() + m.left + cross_off,
                cursor,
                cross_size,
                main_size,
            ),
            Axis::Horizontal => Rect::new(
                cursor,
                content.top() + m.top + cross_off,
                main_size,
                cross_size,
            ),
        };
        layout_node(child.as_mut(), rect, cv);
        cursor += main_size + main_trail(axis, m) + spacing + gap_extra;
    }
}

// —— 轴向辅助 ——
fn main_of(axis: Axis, s: Size) -> f32 {
    main_of_size(axis, s)
}
fn main_of_size(axis: Axis, s: Size) -> f32 {
    match axis {
        Axis::Vertical => s.height,
        Axis::Horizontal => s.width,
    }
}
fn cross_of(axis: Axis, s: Size) -> f32 {
    cross_of_size(axis, s)
}
fn cross_of_size(axis: Axis, s: Size) -> f32 {
    match axis {
        Axis::Vertical => s.width,
        Axis::Horizontal => s.height,
    }
}
fn main_start(axis: Axis, content: Rect) -> f32 {
    match axis {
        Axis::Vertical => content.top(),
        Axis::Horizontal => content.left(),
    }
}
fn main_sizing(axis: Axis, b: &Base) -> Sizing {
    match axis {
        Axis::Vertical => b.height,
        Axis::Horizontal => b.width,
    }
}
fn cross_sizing(axis: Axis, b: &Base) -> Sizing {
    match axis {
        Axis::Vertical => b.width,
        Axis::Horizontal => b.height,
    }
}
fn main_margin(axis: Axis, m: Insets) -> f32 {
    match axis {
        Axis::Vertical => m.vertical(),
        Axis::Horizontal => m.horizontal(),
    }
}
fn cross_margin(axis: Axis, m: Insets) -> f32 {
    match axis {
        Axis::Vertical => m.horizontal(),
        Axis::Horizontal => m.vertical(),
    }
}
fn main_lead(axis: Axis, m: Insets) -> f32 {
    match axis {
        Axis::Vertical => m.top,
        Axis::Horizontal => m.left,
    }
}
fn main_trail(axis: Axis, m: Insets) -> f32 {
    match axis {
        Axis::Vertical => m.bottom,
        Axis::Horizontal => m.right,
    }
}

/// 由「内容尺寸」加上 padding、并应用 Fixed 尺寸覆盖，得到控件最终尺寸。
/// Content/Fill 用计算值（Fill 在容器 arrange 时再撑开）。
pub fn size_from_content(b: &Base, mut w: f32, mut h: f32) -> Size {
    w += b.padding.horizontal();
    h += b.padding.vertical();
    if let Sizing::Fixed(x) = b.width {
        w = x;
    }
    if let Sizing::Fixed(y) = b.height {
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

    // —— 阶段2 新增：Sizing / 对齐 / margin / 绝对定位 ——
    use crate::sizing::{Align, Justify, Sizing};

    #[test]
    fn justify_center_主轴居中() {
        let root = VBox::new()
            .justify(Justify::Center)
            .push(Panel::new().size(50.0, 40.0))
            .push(Panel::new().size(50.0, 40.0));
        let mut root = root;
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 200.0), &cv);
        // free=200-80=120，居中起点=60
        assert_eq!(root.base().children[0].base().rect.top(), 60.0);
        assert_eq!(root.base().children[1].base().rect.top(), 100.0);
    }

    #[test]
    fn justify_space_between_两端对齐() {
        let mut root = VBox::new()
            .justify(Justify::SpaceBetween)
            .push(Panel::new().size(50.0, 40.0))
            .push(Panel::new().size(50.0, 40.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 200.0), &cv);
        assert_eq!(root.base().children[0].base().rect.top(), 0.0);
        // 第二个贴底：200-40=160
        assert_eq!(root.base().children[1].base().rect.top(), 160.0);
    }

    #[test]
    fn align_center_交叉轴居中() {
        let mut root = VBox::new()
            .align(Align::Center)
            .push(Panel::new().size(50.0, 40.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 200.0), &cv);
        // 交叉轴(宽)可用100，子宽50，居中 x=25
        assert_eq!(root.base().children[0].base().rect.left(), 25.0);
        assert_eq!(root.base().children[0].base().rect.size.width, 50.0);
    }

    #[test]
    fn sizing_fill_撑开剩余() {
        let mut fill = Panel::new();
        fill.base_mut().height = Sizing::Fill;
        let mut root = VBox::new().push(Panel::new().size(10.0, 40.0)).push(fill);
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 200.0), &cv);
        assert_eq!(root.base().children[0].base().rect.size.height, 40.0);
        // Fill 拿走剩余 160
        assert_eq!(root.base().children[1].base().rect.size.height, 160.0);
    }

    #[test]
    fn margin_影响排布() {
        let mut root = VBox::new().push(Panel::new().size(50.0, 30.0).margin(10.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 200.0), &cv);
        let c = root.base().children[0].base().rect;
        assert_eq!(c.top(), 10.0); // 上 margin
        assert_eq!(c.left(), 10.0); // 左 margin（Stretch 但去掉左右 margin）
        assert_eq!(c.size.width, 80.0); // 100 - 左右各10
    }

    #[test]
    fn 绝对定位_pos() {
        let mut root = Panel::new().push(Panel::new().size(40.0, 30.0).pos(25.0, 15.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
        let c = root.base().children[0].base().rect;
        assert_eq!(c, Rect::new(25.0, 15.0, 40.0, 30.0));
    }
}
