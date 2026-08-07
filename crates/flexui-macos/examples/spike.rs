//! flexui-rs macOS 演示：用统一 API 搭一个界面，展示
//! 4×2 状态样式、Flex 布局、按钮点击、Radio+TabBox 组成的 tabbar。
//!
//! 运行：`cargo run -p flexui-macos --example spike`

use std::cell::Cell;
use std::rc::Rc;

use flexui_core::{
    BaseState, Button, Color, Dispatcher, HBox, Label, Node, Panel, Radio, StyleSet, StyleSpec,
    TabBox, VBox, Widget,
};
use flexui_core::{Corners, HitPolicy, TextAlign};
use flexui_macos::{run, WindowConfig};

/// 生成一套按钮的分状态样式：normal/hot/pushed/disabled 各不同底色。
fn button_style() -> StyleSet {
    let mk = |bg: Color| StyleSpec {
        bg_color: Some(bg),
        fg_color: Some(Color::WHITE),
        corner_radius: Some(Corners::all(8.0)),
        text_align: Some(TextAlign::Center),
        ..Default::default()
    };
    let mut set = StyleSet::new().with_normal(mk(Color::from_u8(52, 120, 246, 255)));
    set.set(state(BaseState::Hot), mk(Color::from_u8(74, 140, 255, 255)));
    set.set(state(BaseState::Pushed), mk(Color::from_u8(38, 92, 200, 255)));
    set.set(state(BaseState::Disabled), mk(Color::from_u8(120, 128, 140, 255)));
    set
}

fn state(b: BaseState) -> flexui_core::VisualState {
    flexui_core::VisualState::new(b, false)
}

/// 面板背景样式。
fn panel_style(c: Color) -> StyleSet {
    StyleSet::new().with_normal(StyleSpec {
        bg_color: Some(c),
        corner_radius: Some(Corners::all(10.0)),
        ..Default::default()
    })
}

/// 标签样式（前景色）。
fn label_style(c: Color) -> StyleSet {
    StyleSet::new().with_normal(StyleSpec {
        fg_color: Some(c),
        ..Default::default()
    })
}

fn build_ui() -> (Node, Dispatcher) {
    let clicks = Rc::new(Cell::new(0u32));

    // 顶部标题
    let title = Label::new("flexui-rs · 控件/状态/布局/Tabbar 演示")
        .style(label_style(Color::WHITE))
        .height(28.0);

    // 一排按钮：普通 + 禁用
    let c2 = clicks.clone();
    let btn_ok = Button::new("点我 (+1)")
        .style(button_style())
        .size(140.0, 44.0)
        .on_click(move || {
            c2.set(c2.get() + 1);
            println!("[demo] 按钮点击，总计 {}", c2.get());
        });
    let btn_disabled = Button::new("禁用按钮")
        .style(button_style())
        .size(120.0, 44.0)
        .enabled(false);
    let button_row = HBox::new().spacing(12.0).push(btn_ok).push(btn_disabled).height(44.0);

    // Radio 组（tab_index 0/1/2）作为 tabbar 的标签
    let radios = HBox::new()
        .spacing(8.0)
        .height(26.0)
        .push(Radio::new("页面一").group(1).tab_index(0).selected(true).size(90.0, 24.0).style(label_style(Color::WHITE)))
        .push(Radio::new("页面二").group(1).tab_index(1).size(90.0, 24.0).style(label_style(Color::WHITE)))
        .push(Radio::new("页面三").group(1).tab_index(2).size(90.0, 24.0).style(label_style(Color::WHITE)));

    // TabBox：三页，不同底色 + 文案
    let tab = TabBox::new()
        .page(page("第一页内容", Color::from_u8(44, 62, 90, 255)))
        .page(page("第二页内容", Color::from_u8(60, 48, 84, 255)))
        .page(page("第三页内容", Color::from_u8(44, 74, 62, 255)))
        .flex(1.0);
    let tab_id = tab.base().id;

    // 根布局：纵向堆叠，深色背景
    let root = VBox::new()
        .spacing(14.0)
        .padding(20.0)
        .style(panel_style(Color::from_u8(26, 28, 34, 255)))
        .push(title)
        .push(button_row)
        .push(radios)
        .push(tab);

    // 分发器：注册 radio 组 → tabbox 绑定，组成 tabbar
    let mut disp = Dispatcher::new();
    disp.bind_tab(1, tab_id);

    (Box::new(root), disp)
}

/// 造一页：带背景色的面板 + 居中标签。
fn page(text: &str, bg: Color) -> Panel {
    Panel::new()
        .style(panel_style(bg))
        .hit(HitPolicy::Solid)
        .push(
            Label::new(text)
                .style(StyleSet::new().with_normal(StyleSpec {
                    fg_color: Some(Color::WHITE),
                    text_align: Some(TextAlign::Center),
                    ..Default::default()
                })),
        )
}

fn main() {
    let (root, disp) = build_ui();
    run(
        WindowConfig {
            title: "flexui-rs 演示 (macOS)".to_string(),
            width: 640.0,
            height: 460.0,
        },
        root,
        disp,
    );
}
