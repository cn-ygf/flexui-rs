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
    let b = node.base();
    if !b.visible {
        return;
    }
    let style = b.resolved_style();
    let rect = b.rect;
    let radius = style.corner_radius.unwrap_or_default();
    let rounded = !is_zero_corners(radius);

    // 1. 背景色
    if let Some(bg) = style.bg_color {
        if rounded {
            cv.fill_round_rect(rect, radius, bg);
        } else {
            cv.fill_rect(rect, bg);
        }
    }
    // 2. 背景图（支持换色 tint 与渲染方式 fit）
    if let Some(img) = &style.bg_image {
        cv.draw_image(img, rect, style.bg_tint, style.bg_fit.clone().unwrap_or_default());
    }
    // 3. 控件内容（文字/图标）
    node.paint_content(cv, &style);
    // 4. 前景图
    if let Some(img) = &style.fg_image {
        cv.draw_image(img, rect, style.fg_tint, style.fg_fit.clone().unwrap_or_default());
    }
    // 5. 边框
    if let (Some(bc), Some(bw)) = (style.border_color, style.border_width) {
        if bw > 0.0 {
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
        cv.clip_rect(rect);
        for child in b.children.iter() {
            paint_tree(child.as_ref(), cv);
        }
        cv.restore();
    }
}

fn is_zero_corners(c: Corners) -> bool {
    c.tl == 0.0 && c.tr == 0.0 && c.br == 0.0 && c.bl == 0.0
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
    use crate::widgets::{Button, VBox};
    use flexui_geometry::{Color, Rect, Size};
    use flexui_gfx::{Canvas, Font};

    /// 记录绘制调用的假画布，用于断言绘制管线行为。
    #[derive(Default)]
    struct Recorder {
        fills: Vec<(Rect, Color)>,
        texts: Vec<String>,
    }
    impl Canvas for Recorder {
        fn fill_rect(&mut self, r: Rect, c: Color) {
            self.fills.push((r, c));
        }
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, r: Rect, _rad: flexui_geometry::Corners, c: Color) {
            self.fills.push((r, c));
        }
        fn stroke_round_rect(&mut self, _r: Rect, _rad: flexui_geometry::Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, t: &str, _o: flexui_geometry::Point, _f: &Font, _c: Color) {
            self.texts.push(t.to_string());
        }
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
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
