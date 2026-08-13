//! 统一绘制管线（L3）。
//!
//! 对每个控件按「生效样式」统一绘制：背景色 → 背景图 → 内容(控件自绘) → 前景图
//! → 边框 → 子控件(裁剪)。控件本身只需实现 `paint_content`，即可自动获得
//! 4×2 状态下的分状态样式渲染（对应需求 C3）。

use flexui_geometry::{Corners, Rect};
use flexui_gfx::Canvas;

use crate::widget::Widget;

/// 递归绘制整棵控件树。
pub fn paint_tree(node: &dyn Widget, cv: &mut dyn Canvas) {
    paint_tree_impl(node, cv, None, false);
}

/// 只绘制与脏矩形相交的控件分支；画布仍应由后端裁剪到同一区域。
pub fn paint_tree_in_rect(node: &dyn Widget, cv: &mut dyn Canvas, dirty: Rect) {
    paint_tree_impl(node, cv, Some(dirty), false);
}

fn subtree_focused(node: &dyn Widget) -> bool {
    node.base().focused || node.base().children.iter().any(|child| subtree_focused(child.as_ref()))
}

fn paint_tree_impl(
    node: &dyn Widget,
    cv: &mut dyn Canvas,
    dirty: Option<Rect>,
    inherited_focus: bool,
) {
    let b = node.base();
    if !b.visible {
        return;
    }
    let focus_active = inherited_focus || b.focused || (b.focus_within && subtree_focused(node));
    let state = crate::style::VisualState::with_selected(
        b.effective_base(),
        focus_active,
        b.selected,
    );
    let style = b.style.resolve(state);
    let rect = b.rect;
    if let Some(dirty) = dirty {
        let visual_rect = style.shadow.map_or(rect, |shadow| {
            union_rect(
                rect,
                Rect::new(
                    rect.left() + shadow.dx,
                    rect.top() + shadow.dy,
                    rect.size.width,
                    rect.size.height,
                ),
            )
        });
        if !rects_intersect(visual_rect, dirty) {
            return;
        }
    }
    let radius = style.corner_radius.unwrap_or_default();
    let rounded = !is_zero_corners(radius);
    // 控件自身透明度：乘进本控件绘制用到的所有颜色的 alpha（不含子控件）。
    let op = style.opacity.unwrap_or(1.0).clamp(0.0, 1.0);

    // 0. 投影（在背景之下，按 dx/dy 偏移同形填充）。
    if let Some(sh) = style.shadow {
        let sr = Rect::new(rect.left() + sh.dx, rect.top() + sh.dy, rect.size.width, rect.size.height);
        cv.fill_round_rect(sr, radius, dim(sh.color, op));
    }
    // 圆角控件的图片、内容和子树必须使用同一裁剪区；阴影保留在裁剪外。
    if rounded {
        cv.save();
        cv.clip_round_rect(rect, radius);
    }
    // 1. 背景：渐变优先于纯色。
    if let Some(g) = style.gradient {
        cv.fill_gradient_rect(rect, radius, dim(g.from, op), dim(g.to, op), g.vertical);
    } else if let Some(bg) = style.bg_color {
        let c = dim(bg, op);
        if rounded {
            cv.fill_round_rect(rect, radius, c);
        } else {
            cv.fill_rect(rect, c);
        }
    }
    // 2. 背景图（支持换色 tint 与渲染方式 fit）
    let bg_image = b.click_bg_frame_player.image()
        .or_else(|| b.bg_frame_player.image_for_state(style.bg_animation.as_ref()))
        .or(style.bg_image.as_ref());
    if let Some(img) = bg_image {
        cv.draw_image(img, rect, style.bg_tint, style.bg_fit.clone().unwrap_or_default());
    }
    // 3. 控件内容（文字/图标）；透明时用降低 alpha 的前景色。
    if op < 1.0 {
        let mut cs = style.clone();
        cs.fg_color = cs.fg_color.map(|c| dim(c, op));
        node.paint_content(cv, &cs);
    } else {
        node.paint_content(cv, &style);
    }
    // 4. 前景图
    let fg_image = b.click_fg_frame_player.image()
        .or_else(|| b.fg_frame_player.image_for_state(style.fg_animation.as_ref()))
        .or(style.fg_image.as_ref());
    if let Some(img) = fg_image {
        cv.draw_image(img, rect, style.fg_tint, style.fg_fit.clone().unwrap_or_default());
    }
    // 5. 边框
    if let (Some(bc), Some(bw)) = (style.border_color, style.border_width) {
        if bw > 0.0 {
            let bc = dim(bc, op);
            if rounded {
                cv.stroke_round_rect(rect, radius, bc, bw);
            } else {
                cv.stroke_rect(rect, bc, bw);
            }
        }
    }
    // 6. 子控件（裁剪到自身范围内）
    if !b.children.is_empty() {
        cv.save();
        cv.clip_rect(node.children_viewport());
        for child in b.children.iter() {
            paint_tree_impl(child.as_ref(), cv, dirty, focus_active && b.focus_within);
        }
        cv.restore();
    }
    // 7. 前景覆盖层（如滚动条）。
    node.paint_foreground(cv, &style);
    if rounded {
        cv.restore();
    }
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.left() < b.right() && a.right() > b.left() && a.top() < b.bottom() && a.bottom() > b.top()
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let left = a.left().min(b.left());
    let top = a.top().min(b.top());
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    Rect::new(left, top, right - left, bottom - top)
}

fn is_zero_corners(c: Corners) -> bool {
    c.tl == 0.0 && c.tr == 0.0 && c.br == 0.0 && c.bl == 0.0
}

/// 把颜色的 alpha 乘以透明度系数（op=1 原样返回）。
fn dim(c: flexui_geometry::Color, op: f32) -> flexui_geometry::Color {
    if op >= 1.0 {
        c
    } else {
        flexui_geometry::Color::rgba(c.r, c.g, c.b, c.a * op)
    }
}

/// 便捷：在内容区内按对齐方式画一行文字（控件 paint_content 复用）。
///
/// `elide=true` 时超宽文本尾部用省略号「…」截断（静态文本控件用；Edit 传 false 以免截断输入）。
pub fn draw_aligned_text(
    cv: &mut dyn Canvas,
    text: &str,
    content: Rect,
    font: &flexui_gfx::Font,
    color: flexui_geometry::Color,
    align: flexui_gfx::TextAlign,
    elide: bool,
) {
    use flexui_geometry::Point;
    use flexui_gfx::TextAlign;
    if text.is_empty() {
        return;
    }
    // 超宽则截断加省略号。
    let shown = if elide {
        elide_to_width(cv, text, font, content.size.width)
    } else {
        text.to_string()
    };
    if shown.is_empty() {
        return;
    }
    let size = cv.measure_text(&shown, font);
    let x = match align {
        TextAlign::Left => content.left(),
        TextAlign::Center => content.left() + (content.size.width - size.width) / 2.0,
        TextAlign::Right => content.right() - size.width,
    };
    // 垂直居中
    let y = content.top() + (content.size.height - size.height) / 2.0;
    cv.draw_text(&shown, Point::new(x, y.max(content.top())), font, color);
}

/// 把文本尾部用「…」截断到不超过 max_w（单行）。宽度足够则原样返回。
pub fn elide_to_width(
    cv: &dyn Canvas,
    text: &str,
    font: &flexui_gfx::Font,
    max_w: f32,
) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if cv.measure_text(text, font).width <= max_w {
        return text.to_string();
    }
    const ELL: &str = "…";
    let chars: Vec<char> = text.chars().collect();
    // 从长到短找到「前缀 + …」能放下的最大前缀。
    let mut n = chars.len();
    while n > 0 {
        n -= 1;
        let mut s: String = chars[..n].iter().collect();
        s.push_str(ELL);
        if cv.measure_text(&s, font).width <= max_w {
            return s;
        }
    }
    ELL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_node;
    use crate::style::{BaseState, StyleSet, StyleSpec, VisualState};
    use crate::widgets::{Button, Panel, VBox};
    use flexui_geometry::{Color, Rect, Size};
    use flexui_gfx::{Canvas, Font};

    /// 记录绘制调用的假画布，用于断言绘制管线行为。
    #[derive(Default)]
    struct Recorder {
        fills: Vec<(Rect, Color)>,
        strokes: Vec<Color>,
        texts: Vec<String>,
        round_clips: Vec<(Rect, flexui_geometry::Corners)>,
    }
    impl Canvas for Recorder {
        fn fill_rect(&mut self, r: Rect, c: Color) {
            self.fills.push((r, c));
        }
        fn stroke_rect(&mut self, _r: Rect, c: Color, _w: f32) {
            self.strokes.push(c);
        }
        fn fill_round_rect(&mut self, r: Rect, _rad: flexui_geometry::Corners, c: Color) {
            self.fills.push((r, c));
        }
        fn stroke_round_rect(&mut self, _r: Rect, _rad: flexui_geometry::Corners, c: Color, _w: f32) {
            self.strokes.push(c);
        }
        fn draw_text(&mut self, t: &str, _o: flexui_geometry::Point, _f: &Font, _c: Color) {
            self.texts.push(t.to_string());
        }
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
        fn clip_round_rect(&mut self, rect: Rect, radius: flexui_geometry::Corners) {
            self.round_clips.push((rect, radius));
        }
    }

    fn button_with_states() -> Button {
        let normal = StyleSpec {
            bg_color: Some(Color::from_u8(10, 10, 10, 255)),
            fg_color: Some(Color::WHITE),
            ..Default::default()
        };
        let hot = StyleSpec {
            bg_color: Some(Color::from_u8(200, 200, 200, 255)),
            ..Default::default()
        };
        let mut set = StyleSet::new().with_normal(normal);
        set.set(VisualState::new(BaseState::Hot, false), hot);
        Button::new("hi").size(100.0, 40.0).style(set)
    }

    #[test]
    fn 绘制管线_按状态选用背景色并画文字() {
        let mut root = VBox::new().push(button_with_states());
        let mut rec = Recorder::default();
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &rec);

        // normal 状态：应出现 normal 底色 + 文字
        paint_tree(&root, &mut rec);
        assert!(rec.fills.iter().any(|(_, c)| *c == Color::from_u8(10, 10, 10, 255)), "应画 normal 底色");
        assert!(rec.texts.iter().any(|t| t == "hi"), "应画按钮文字");

        // 置 hover：应改用 hot 底色
        root.base_mut().children[0].base_mut().hover = true;
        let mut rec2 = Recorder::default();
        paint_tree(&root, &mut rec2);
        assert!(rec2.fills.iter().any(|(_, c)| *c == Color::from_u8(200, 200, 200, 255)), "hover 应用 hot 底色");
    }

    #[test]
    fn 透明度_降低填充alpha() {
        let spec = StyleSpec {
            bg_color: Some(Color::rgba(1.0, 0.0, 0.0, 1.0)),
            opacity: Some(0.5),
            ..Default::default()
        };
        let panel = Panel::new().style(StyleSet::new().with_normal(spec)).size(10.0, 10.0);
        let mut root = VBox::new().push(panel);
        let mut rec = Recorder::default();
        layout_node(&mut root, Rect::new(0.0, 0.0, 50.0, 50.0), &rec);
        paint_tree(&root, &mut rec);
        // 应有一次红色、alpha≈0.5 的填充。
        assert!(
            rec.fills.iter().any(|(_, c)| c.r == 1.0 && (c.a - 0.5).abs() < 1e-6),
            "透明度应把红色 alpha 降到 0.5"
        );
    }

    #[test]
    fn 圆角控件_裁剪图片内容与子树() {
        let radius = flexui_geometry::Corners::all(8.0);
        let panel = Panel::new()
            .style(StyleSet::new().with_normal(StyleSpec {
                corner_radius: Some(radius),
                ..Default::default()
            }))
            .size(40.0, 30.0);
        let mut root = VBox::new().push(panel);
        let mut rec = Recorder::default();
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &rec);
        paint_tree(&root, &mut rec);
        assert_eq!(rec.round_clips.len(), 1);
        assert_eq!(rec.round_clips[0].1, radius);
    }

    #[test]
    fn 区域绘制_跳过脏区外控件() {
        let mut root = VBox::new()
            .push(button_with_states())
            .push(button_with_states());
        let layout = Recorder::default();
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &layout);

        let mut rec = Recorder::default();
        paint_tree_in_rect(&root, &mut rec, Rect::new(0.0, 0.0, 100.0, 40.0));

        assert_eq!(rec.texts, ["hi"], "只应绘制与脏区相交的第一个按钮");
    }

    #[test]
    fn 区域绘制_保留与脏区相交的阴影控件() {
        let panel = Panel::new()
            .style(StyleSet::new().with_normal(StyleSpec {
                bg_color: Some(Color::BLACK),
                shadow: Some(crate::style::Shadow {
                    dx: 10.0,
                    dy: 0.0,
                    color: Color::BLACK,
                }),
                ..Default::default()
            }))
            .size(40.0, 30.0);
        let mut root = VBox::new().push(panel);
        let layout = Recorder::default();
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &layout);

        let mut rec = Recorder::default();
        paint_tree_in_rect(&root, &mut rec, Rect::new(45.0, 0.0, 5.0, 30.0));

        assert!(!rec.fills.is_empty(), "阴影与脏区相交时不能跳过控件");
    }

    #[test]
    fn focus_within_后代焦点驱动容器及子树样式() {
        let normal = Color::from_u8(20, 20, 20, 255);
        let focused = Color::from_u8(0, 210, 196, 255);
        let mut styles = StyleSet::new().with_normal(StyleSpec {
            border_color: Some(normal),
            border_width: Some(1.0),
            ..Default::default()
        });
        styles.set(
            VisualState::new(BaseState::Normal, true),
            StyleSpec {
                border_color: Some(focused),
                border_width: Some(1.0),
                ..Default::default()
            },
        );
        let mut container = Panel::new().style(styles).size(100.0, 40.0);
        container.base_mut().focus_within = true;
        let mut child = button_with_states();
        child.base_mut().focused = true;
        container.base_mut().children.push(Box::new(child));
        let mut root = VBox::new().push(container);
        let mut rec = Recorder::default();
        layout_node(&mut root, Rect::new(0.0, 0.0, 120.0, 60.0), &rec);

        paint_tree(&root, &mut rec);

        assert!(rec.strokes.contains(&focused), "后代获焦时容器应使用 focus 样式");
    }

    #[test]
    fn 省略号_超宽文本截断() {
        use flexui_gfx::TextAlign;
        let mut rec = Recorder::default();
        let font = Font::default(); // size14 → 每字 8.4px
        // 内容宽 30 → 放不下 8 个字，应截断加「…」。
        draw_aligned_text(
            &mut rec,
            "abcdefgh",
            Rect::new(0.0, 0.0, 30.0, 20.0),
            &font,
            Color::BLACK,
            TextAlign::Left,
            true,
        );
        let shown = &rec.texts[0];
        assert!(shown.ends_with('…'), "应以省略号结尾: {shown}");
        assert!(shown.chars().count() < 8, "应比原文短");
        assert!(rec.measure_text(shown, &font).width <= 30.0, "应放得下");
    }

    #[test]
    fn 省略号_关闭时不截断() {
        use flexui_gfx::TextAlign;
        let mut rec = Recorder::default();
        let font = Font::default();
        draw_aligned_text(
            &mut rec,
            "abcdefgh",
            Rect::new(0.0, 0.0, 30.0, 20.0),
            &font,
            Color::BLACK,
            TextAlign::Left,
            false,
        );
        assert_eq!(rec.texts[0], "abcdefgh");
    }
}
