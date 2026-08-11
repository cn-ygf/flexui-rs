//! 轻量性能基准（无第三方依赖）：度量布局 + 绘制管线在中等规模控件树上的吞吐。
//!
//! 运行：`cargo run -p flexui-core --example bench --release`
//! 用 no-op 画布，只测布局/分发/绘制遍历本身（不含真实光栅化）。

use std::time::Instant;

use flexui_core::{layout_node, paint_tree, Button, Canvas, HBox, Label, VBox};
use flexui_geometry::{Color, Corners, Point, Rect, Size};
use flexui_gfx::Font;

/// 什么都不画的画布：只让布局/绘制遍历真实执行。
struct NullCanvas;
impl Canvas for NullCanvas {
    fn fill_rect(&mut self, _r: Rect, _c: Color) {}
    fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
    fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
    fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
    fn draw_text(&mut self, _t: &str, _o: Point, _f: &Font, _c: Color) {}
    fn measure_text(&self, t: &str, f: &Font) -> Size {
        Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
    }
}

fn build_tree(rows: usize) -> VBox {
    let mut root = VBox::new().spacing(4.0).padding(8.0);
    for i in 0..rows {
        let row = HBox::new()
            .spacing(8.0)
            .push(Label::new(format!("第 {i} 行标签")).width(160.0).height(24.0))
            .push(Button::new("操作").width(80.0).height(24.0))
            .push(Label::new("说明文字，可能较长会被省略号截断").flex(1.0).height(24.0));
        root = root.push(row);
    }
    root
}

fn main() {
    let rows = 300;
    let iters = 2000;
    let mut root = build_tree(rows);
    let widgets = rows * 4 + 1; // 估算控件数
    let mut cv = NullCanvas;
    let area = Rect::new(0.0, 0.0, 900.0, 12.0 * rows as f32);

    // 预热。
    for _ in 0..50 {
        layout_node(&mut root, area, &cv);
        paint_tree(&root, &mut cv);
    }

    let t0 = Instant::now();
    for _ in 0..iters {
        layout_node(&mut root, area, &cv);
        paint_tree(&root, &mut cv);
    }
    let dt = t0.elapsed();

    let per = dt.as_secs_f64() * 1e6 / iters as f64;
    println!("控件数≈{widgets}  迭代 {iters} 次（布局+绘制遍历）");
    println!("总耗时 {:?}  每帧 {:.1} µs  ≈ {:.0} fps", dt, per, 1e6 / per);
}
