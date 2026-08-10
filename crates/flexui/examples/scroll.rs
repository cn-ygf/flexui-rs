//! ScrollView 滚动容器演示（阶段5 P0）。
//! 运行：`cargo run -p flexui --example scroll`
//! 鼠标滚轮 / 触控板上下滚动列表；Tab 键在可聚焦控件间遍历。

use flexui::{
    Color, Corners, Label, Panel, ScrollView, Skin, StyleSet, StyleSpec, VBox, Window, WindowConfig,
    WindowImpl,
};

fn item_style(i: usize) -> StyleSet {
    let bg = if i.is_multiple_of(2) {
        Color::from_u8(44, 48, 60, 255)
    } else {
        Color::from_u8(36, 40, 52, 255)
    };
    StyleSet::new().with_normal(StyleSpec {
        bg_color: Some(bg),
        corner_radius: Some(Corners::all(6.0)),
        ..Default::default()
    })
}

fn label_style(c: Color) -> StyleSet {
    StyleSet::new().with_normal(StyleSpec {
        fg_color: Some(c),
        ..Default::default()
    })
}

struct ScrollWin;

impl WindowImpl for ScrollWin {
    fn config(&self) -> WindowConfig {
        WindowConfig::new("flexui-rs · ScrollView", 420.0, 360.0)
    }
    fn skin(&self) -> Skin {
        let mut sv = ScrollView::new().spacing(6.0).flex(1.0);
        for i in 1..=25 {
            sv = sv.push(
                Panel::new().height(44.0).padding(10.0).style(item_style(i)).push(
                    Label::new(format!("列表项 {i} —— 滚动查看更多"))
                        .style(label_style(Color::from_u8(230, 235, 245, 255))),
                ),
            );
        }
        let root = VBox::new()
            .padding(12.0)
            .spacing(8.0)
            .style(StyleSet::new().with_normal(StyleSpec {
                bg_color: Some(Color::from_u8(26, 28, 34, 255)),
                ..Default::default()
            }))
            .push(
                Label::new("滚动列表（滚轮/触控板上下滚）")
                    .height(24.0)
                    .style(label_style(Color::WHITE)),
            )
            .push(sv);
        Skin::tree(Box::new(root))
    }
}

fn main() {
    Window::new(ScrollWin).run();
}
