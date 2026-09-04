use super::*;
use crate::event::{Mods, MouseButton};
use crate::layout::layout_node;
use crate::widgets::{
    Button, CheckBox, ComboBox, Edit, Label, ListView, Panel, Progress, Radio, ScrollView, Slider,
    TabBox, VBox,
};
use crate::WidgetProperty;
use flexui_gfx::{Canvas, Font};
use flexui_gfx::{Corners, Rect, Size};
use std::cell::Cell;
use std::rc::Rc;

struct FakeCanvas;
impl Canvas for FakeCanvas {
    fn fill_rect(&mut self, _r: Rect, _c: flexui_gfx::Color) {}
    fn stroke_rect(&mut self, _r: Rect, _c: flexui_gfx::Color, _w: f32) {}
    fn fill_round_rect(&mut self, _r: Rect, _rad: flexui_gfx::Corners, _c: flexui_gfx::Color) {}
    fn stroke_round_rect(
        &mut self,
        _r: Rect,
        _rad: flexui_gfx::Corners,
        _c: flexui_gfx::Color,
        _w: f32,
    ) {
    }
    fn draw_text(&mut self, _t: &str, _o: flexui_gfx::Point, _f: &Font, _c: flexui_gfx::Color) {}
    fn measure_text(&self, t: &str, f: &Font) -> Size {
        Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
    }
}

/// 构造无修饰键的 KeyDown（测试便捷）。
fn kd(key: u32) -> Event {
    Event::KeyDown {
        key,
        mods: Mods::default(),
    }
}

fn click_at(disp: &mut Dispatcher, root: &mut dyn Widget, p: Point) {
    disp.handle(
        root,
        &Event::MouseDown {
            pos: p,
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    disp.handle(
        root,
        &Event::MouseUp {
            pos: p,
            button: MouseButton::Left,
        },
    );
}

#[test]
fn scrollview_滚轮滚动() {
    let mut root = ScrollView::new()
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0)); // 5×40 = 200 内容高
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
    assert_eq!(root.base().children[0].base().rect.top(), 0.0);

    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::MouseWheel {
            pos: Point::new(50.0, 50.0),
            dx: 0.0,
            dy: -60.0,
        },
    );
    assert_eq!(root.scroll_offset(), Some(Point::new(0.0, 60.0))); // 视口100 内容200 → 可滚到 100，60 有效
                                                                   // 脏区须覆盖控件整体（含右侧滚动条列），否则滑块不重绘。
    let dirty = disp.dirty.expect("滚轮滚动应产生脏区");
    assert!(
        dirty.right() >= root.base().rect.right() - 0.01,
        "脏区右边界须到达控件右缘以重绘滚动条：{dirty:?}"
    );
    // 重新布局后首个子应上移 60
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
    assert_eq!(root.base().children[0].base().rect.top(), -60.0);
}

#[test]
fn scrollview_拖动滚动条() {
    let mut root = ScrollView::new()
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0))
        .push(Panel::new().size(80.0, 40.0)); // 5×40 = 200 内容高，视口 100
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);

    let mut disp = Dispatcher::new();
    // 滑块 x∈[93,98]（宽5、边距2），初始 y∈[0,50]（thumb_h=50）。按在 (95,10)。
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(95.0, 10.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    // 拖到 y=40：滑块顶=30，行程=50，t=0.6，max=100 → 偏移 60。
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(95.0, 40.0),
        },
    );
    let dragged = root.scroll_offset().unwrap().y;
    assert!(
        (dragged - 60.0).abs() < 0.01,
        "拖动后偏移应≈60，实为 {dragged}"
    );
    // 子树立即随拖动上移。
    assert!((root.base().children[0].base().rect.top() - (-60.0)).abs() < 0.01);
    // 抬起后结束拖动，再移动不再改变偏移。
    disp.handle(
        &mut root,
        &Event::MouseUp {
            pos: Point::new(95.0, 40.0),
            button: MouseButton::Left,
        },
    );
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(95.0, 80.0),
        },
    );
    assert!(
        (root.scroll_offset().unwrap().y - dragged).abs() < 0.01,
        "抬起后不应再变"
    );
}

#[test]
fn 双击与右键记录() {
    let mut root = VBox::new().push(Button::new("x").name("b").size(50.0, 30.0));
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::DoubleClick {
            pos: Point::new(25.0, 15.0),
        },
    );
    assert_eq!(disp.take_double_clicks(), vec!["b".to_string()]);
    disp.handle(
        &mut root,
        &Event::MouseUp {
            pos: Point::new(25.0, 15.0),
            button: MouseButton::Right,
        },
    );
    let ctx = disp.take_context_clicks();
    assert_eq!(ctx.len(), 1);
    assert_eq!(ctx[0].0, "b");
}

#[test]
fn 光标_滚动条区域用箭头() {
    use crate::scroll::ScrollBarVisibility;
    use crate::widgets::Edit;
    let cv = FakeCanvas;
    let mut edit = Edit::new()
        .multiline(true)
        .scrollbar(ScrollBarVisibility::Always);
    edit.set_text_value("a\nb\nc\nd\ne\nf\ng\nh".into());
    layout_node(&mut edit, Rect::new(0.0, 0.0, 120.0, 40.0), &cv);
    // 文本区 → 文本 I-beam。
    assert!(point_wants_text_cursor(&edit, Point::new(10.0, 10.0)));
    // 右缘滚动条区 → 箭头（false）。
    assert!(!point_wants_text_cursor(&edit, Point::new(115.0, 20.0)));
}

#[test]
fn edit_光标编辑() {
    use crate::event::keys;
    use crate::widgets::Edit;
    let mut e = Edit::new();
    for ch in "abc".chars() {
        e.on_event(&Event::Char { ch });
    }
    assert_eq!(e.base().text, "abc");
    // 左移后插入 X → abXc
    e.on_event(&kd(keys::LEFT));
    e.on_event(&Event::Char { ch: 'X' });
    assert_eq!(e.base().text, "abXc");
    // Home + Delete 删首 → bXc
    e.on_event(&kd(keys::HOME));
    e.on_event(&kd(keys::DELETE));
    assert_eq!(e.base().text, "bXc");
    // End + Backspace 删末 → bX
    e.on_event(&kd(keys::END));
    e.on_event(&kd(keys::BACKSPACE));
    assert_eq!(e.base().text, "bX");
}

#[test]
fn edit_鼠标点击拖拽选区() {
    // FakeCanvas 下每字符宽 = 14*0.6 = 8.4；边界 i 在 x=8.4i。
    let mut root = Edit::new().text("hello");
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    let mut disp = Dispatcher::new();
    // 点击 x≈25 → 边界3；光标定位、锚点同处（无选区）。
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(25.0, 20.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    assert_eq!(root.cursor(), 3);
    assert_eq!(root.selection(), None);
    // 拖到 x≈8 → 边界1；选区 (1,3) = "el"。
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(8.0, 20.0),
        },
    );
    assert_eq!(root.selection(), Some((1, 3)));
    assert_eq!(root.selected_text().as_deref(), Some("el"));
}

#[test]
fn edit_连续输入时光标常亮并延后闪烁() {
    let mut root = Edit::new().text("hello");
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(25.0, 20.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );

    let started = Instant::now();
    disp.reset_caret_blink_at(&mut root, started);
    assert!(root.base().caret_on);
    assert!(disp
        .blink_at(
            &mut root,
            started + CARET_BLINK_RESUME_DELAY - Duration::from_millis(1)
        )
        .is_none());
    assert!(root.base().caret_on);

    let typed_again = started + Duration::from_millis(400);
    disp.reset_caret_blink_at(&mut root, typed_again);
    assert!(
        disp.blink_at(
            &mut root,
            started + CARET_BLINK_RESUME_DELAY + Duration::from_millis(1),
        )
        .is_none(),
        "后续输入必须重新开始静止期"
    );
    assert!(root.base().caret_on);

    assert!(disp
        .blink_at(&mut root, typed_again + CARET_BLINK_RESUME_DELAY)
        .is_some());
    assert!(!root.base().caret_on);
}

#[test]
fn 禁用的指针控件不接收鼠标事件() {
    let mut root = Edit::new().text("hello");
    root.base_mut().enabled = false;
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    let mut disp = Dispatcher::new();

    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(0.0, 20.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(8.0, 20.0),
        },
    );

    assert_eq!(root.cursor(), 5);
    assert_eq!(root.selection(), None);
    assert!(!root.base().pressed);
    assert!(!root.base().focused);
}

#[test]
fn slider_按下拖动改变值() {
    let mut root = Slider::new().width(100.0).height(20.0);
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 20.0), &cv);
    let mut disp = Dispatcher::new();
    // 按下 x=50 → value≈0.5
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(50.0, 10.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    assert!(
        (root.current() - 0.5).abs() < 1e-3,
        "got {}",
        root.current()
    );
    // 拖到 x=80 → 0.8
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(80.0, 10.0),
        },
    );
    assert!((root.current() - 0.8).abs() < 1e-3);
    // 越界夹取
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(200.0, 10.0),
        },
    );
    assert_eq!(root.current(), 1.0);
}

#[test]
fn 变换后的滑块按视觉坐标交互() {
    let mut root = Slider::new()
        .width(100.0)
        .height(20.0)
        .translate(100.0, 30.0);
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 20.0), &FakeCanvas);
    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(150.0, 40.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    assert!((root.current() - 0.5).abs() < 0.01);
    assert!(hit_test(&root, Point::new(50.0, 10.0)).is_none());
}

#[test]
fn 非矩形命中排除透明角落且保留中心() {
    let mut root = Panel::new();
    root.base_mut().rect = Rect::new(0.0, 0.0, 300.0, 160.0);
    let mut child = Button::new("round")
        .size(100.0, 40.0)
        .translate(100.0, 20.0)
        .hit_shape(crate::HitShape::Rounded(Corners::all(16.0)));
    child.base_mut().rect = Rect::new(20.0, 20.0, 100.0, 40.0);
    let child_id = child.base().id;
    root.base_mut().children.push(Box::new(child));

    assert_eq!(hit_test(&root, Point::new(170.0, 60.0)), Some(child_id));
    assert_ne!(hit_test(&root, Point::new(121.0, 41.0)), Some(child_id));
}

#[test]
fn place_overlay_下方上翻夹取() {
    let win = Size::new(200.0, 100.0);
    // 下方够放：y=锚点底部；宽取 max(desired,min)。
    let r = place_overlay(
        Rect::new(10.0, 10.0, 50.0, 20.0),
        Size::new(40.0, 30.0),
        win,
        50.0,
        flexui_gfx::Insets::default(),
        crate::widgets::MenuAlignment::Start,
    );
    assert_eq!(r.top(), 30.0);
    assert_eq!(r.size.width, 50.0);
    // 下方放不下 → 上翻到锚点上方。
    let r2 = place_overlay(
        Rect::new(10.0, 80.0, 50.0, 15.0),
        Size::new(40.0, 30.0),
        win,
        0.0,
        flexui_gfx::Insets::default(),
        crate::widgets::MenuAlignment::Start,
    );
    assert_eq!(r2.top(), 50.0);
    // 锚点靠右 → X 夹到窗内。
    let r3 = place_overlay(
        Rect::new(180.0, 10.0, 10.0, 10.0),
        Size::new(40.0, 10.0),
        win,
        0.0,
        flexui_gfx::Insets::default(),
        crate::widgets::MenuAlignment::Start,
    );
    assert_eq!(r3.left(), 160.0);
    // 原版登录菜单在 580 宽窗口内保留 14px 右边距。
    let r4 = place_overlay(
        Rect::new(326.0, 116.0, 68.0, 44.0),
        Size::new(294.0, 228.0),
        Size::new(580.0, 416.0),
        68.0,
        flexui_gfx::Insets::new(0.0, 0.0, 14.0, 28.0),
        crate::widgets::MenuAlignment::Start,
    );
    assert_eq!(r4, Rect::new(272.0, 160.0, 294.0, 228.0));
}

#[test]
fn 菜单偏移在自动摆放后生效() {
    let mut disp = Dispatcher::new();
    disp.open_styled_menu(
        Rect::new(150.0, 50.0, 20.0, 20.0),
        vec![("设置".into(), "settings".into())],
        Some(crate::widgets::MenuStyle {
            width: Some(100.0),
            offset: Point::new(10.0, -6.0),
            alignment: crate::widgets::MenuAlignment::End,
            ..Default::default()
        }),
        None,
    );

    disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));

    assert_eq!(disp.overlays[0].root.base().rect.left(), 80.0);
    assert_eq!(disp.overlays[0].root.base().rect.top(), 64.0);
}

#[test]
fn 浮层滚轮只滚菜单且保持打开() {
    let mut root = ScrollView::new()
        .push(Panel::new().height(200.0))
        .height(100.0);
    layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &FakeCanvas);

    let style = crate::widgets::MenuStyle {
        width: Some(160.0),
        height: Some(100.0),
        row_height: 32.0,
        panel_padding: flexui_gfx::Insets::all(4.0),
        ..Default::default()
    };
    let items = (0..8)
        .map(|i| (format!("item {i}"), format!("item_{i}")))
        .collect();
    let mut disp = Dispatcher::new();
    disp.open_styled_menu(Rect::new(20.0, 20.0, 80.0, 20.0), items, Some(style), None);
    disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
    let menu_viewport = disp.overlays[0].root.children_viewport();
    let main_before = root.scroll_offset();
    disp.handle(
        &mut root,
        &Event::MouseWheel {
            pos: Point::new(menu_viewport.left() + 20.0, menu_viewport.top() + 20.0),
            dx: 0.0,
            dy: -32.0,
        },
    );
    assert!(disp.has_overlays());
    assert_eq!(
        disp.overlays[0].root.scroll_offset(),
        Some(Point::new(0.0, 32.0))
    );
    assert_eq!(root.scroll_offset(), main_before);
}

#[test]
fn 滚出子视口的菜单项不能命中() {
    let style = crate::widgets::MenuStyle {
        width: Some(160.0),
        height: Some(100.0),
        row_height: 32.0,
        panel_padding: flexui_gfx::Insets::new(10.0, 16.0, 10.0, 16.0),
        ..Default::default()
    };
    let items = (0..5)
        .map(|i| (format!("item {i}"), format!("item_{i}")))
        .collect::<Vec<_>>();
    let mut menu = crate::widgets::build_menu_styled(&items, None, &style, None);
    layout_node(
        menu.as_mut(),
        Rect::new(0.0, 0.0, 160.0, 100.0),
        &FakeCanvas,
    );
    assert!(menu.scroll_by(0.0, -20.0));
    let padding_point = Point::new(20.0, 10.0);
    assert!(menu.base().children[0].base().rect.contains(padding_point));
    assert_ne!(
        hit_test(menu.as_ref(), padding_point),
        Some(menu.base().children[0].base().id)
    );
}

#[test]
fn combobox_点击弹下拉_选中回填并上报() {
    let mut root = VBox::new().push(
        ComboBox::new()
            .name("cb")
            .options(["A", "B", "C"])
            .size(120.0, 30.0),
    );
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
    let mut disp = Dispatcher::new();
    let cb_rect = root.base().children[0].base().rect;
    // 点击 ComboBox → 打开下拉。
    click_at(
        &mut disp,
        &mut root,
        Point::new(cb_rect.left() + 10.0, cb_rect.top() + 10.0),
    );
    assert!(disp.has_overlays());
    assert_eq!(
        disp.top_overlay_owner(),
        Some(root.base().children[0].base().id)
    );
    let _ = disp.take_activations(); // 忽略点击 combo 本身的激活
                                     // 布局浮层后点击第 2 项 "B"。
    disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
    let c = disp.top_overlay_item_center(1).unwrap();
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: c,
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    disp.handle(
        &mut root,
        &Event::MouseUp {
            pos: c,
            button: MouseButton::Left,
        },
    );
    assert!(!disp.has_overlays(), "选中后关闭");
    assert_eq!(disp.take_activations(), vec!["cb".to_string()]);
    assert_eq!(root.base().children[0].base().text, "B");
}

#[test]
fn 浮层_点外部与esc关闭且不动主树焦点() {
    // 先让一个 Edit 获焦，再打开下拉，验证关闭浮层不清 Edit 焦点。
    let mut root = VBox::new()
        .push(Edit::new().name("e").size(120.0, 30.0))
        .push(
            ComboBox::new()
                .name("cb")
                .options(["A", "B"])
                .size(120.0, 30.0),
        );
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
    let mut disp = Dispatcher::new();
    // 聚焦 Edit。
    click_at(&mut disp, &mut root, Point::new(10.0, 15.0));
    let edit_focus = disp.focus();
    assert!(edit_focus.is_some());
    // 打开下拉（点 combo，位于第二行 y≈45）。
    let cb_rect = root.base().children[1].base().rect;
    click_at(
        &mut disp,
        &mut root,
        Point::new(cb_rect.left() + 10.0, cb_rect.top() + 10.0),
    );
    assert!(disp.has_overlays());
    // 点浮层外部 → 关闭；主树焦点不变。
    disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(280.0, 280.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    assert!(!disp.has_overlays(), "点外部关闭");
    // 再开一次，用 ESC 关。
    click_at(
        &mut disp,
        &mut root,
        Point::new(cb_rect.left() + 10.0, cb_rect.top() + 10.0),
    );
    assert!(disp.has_overlays());
    disp.handle(&mut root, &kd(crate::event::keys::ESCAPE));
    assert!(!disp.has_overlays(), "ESC 关闭");
}

#[test]
fn 右键菜单_选项按项名上报() {
    let mut root = VBox::new().push(Panel::new().size(100.0, 100.0));
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
    let mut disp = Dispatcher::new();
    disp.open_menu(
        Rect::new(10.0, 10.0, 0.0, 0.0),
        vec![
            ("复制".to_string(), "copy".to_string()),
            ("粘贴".to_string(), "paste".to_string()),
        ],
    );
    assert!(disp.has_overlays());
    disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
    // 点第 2 项 "粘贴"。
    let c = disp.top_overlay_item_center(1).unwrap();
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: c,
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    disp.handle(
        &mut root,
        &Event::MouseUp {
            pos: c,
            button: MouseButton::Left,
        },
    );
    assert!(!disp.has_overlays());
    assert_eq!(disp.take_activations(), vec!["paste".to_string()]);
}

#[test]
fn listview_点击选中并上报激活() {
    let mut root = VBox::new().push(
        ListView::new()
            .name("lv")
            .items(["a", "b", "c"])
            .row_height(20.0),
    );
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
    let mut disp = Dispatcher::new();
    // 点击第 2 行（y≈50 → row 2，列表在 (0,0,200,200)）。
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(20.0, 50.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    assert_eq!(disp.take_activations(), vec!["lv".to_string()]);
    assert_eq!(root.base().children[0].selected_index(), Some(2));
}

#[test]
fn 主线程邮箱_跨线程投递与取走() {
    let disp = Dispatcher::new();
    let p = disp.proxy();
    assert!(p.send("a"));
    let p2 = p.clone();
    assert!(std::thread::spawn(move || p2.send("b")).join().unwrap());
    let mut msgs = disp.drain_messages();
    msgs.sort();
    assert_eq!(msgs, vec!["a".to_string(), "b".to_string()]);
    assert!(disp.drain_messages().is_empty(), "取走后清空");
}

#[test]
fn 主线程任务_可从工作线程投递并通过窗口上下文修改属性() {
    struct TestWindow;
    impl crate::window::WindowHandle for TestWindow {
        fn set_title(&mut self, _title: &str) {}
        fn close(&mut self) {}
        fn minimize(&mut self) {}
        fn maximize(&mut self) {}
        fn restore(&mut self) {}
    }

    let disp = Dispatcher::new();
    let proxy = disp.proxy();
    assert!(std::thread::spawn(move || {
        proxy.post(|ctx| {
            ctx.set_text("status", "loaded");
            ctx.set_enabled("status", false);
        })
    })
    .join()
    .unwrap());

    let mut root = Label::new("waiting").name("status");
    let mut window = TestWindow;
    let mut ctx = crate::window::WindowCtx::new(&mut root, &mut window);
    for task in disp.drain_ui_tasks() {
        task(&mut ctx);
    }
    let invalidation = ctx.take_invalidation();
    drop(ctx);
    assert_eq!(root.base().text, "loaded");
    assert!(!root.base().enabled);
    assert_eq!(invalidation, Invalidation::Layout);
    assert!(disp.drain_ui_tasks().is_empty(), "任务只能执行一次");
}

#[test]
fn 主线程代理_窗口销毁后拒绝新任务() {
    let disp = Dispatcher::new();
    let proxy = disp.proxy();
    drop(disp);
    assert!(!proxy.send("late"));
    assert!(!proxy.post(|ctx| ctx.request_redraw()));
}

#[test]
fn 动画_value线性补间() {
    let mut root = VBox::new().push(Progress::new().name("p").value(0.0));
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    let mut disp = Dispatcher::new();
    assert!(disp.animate(&mut root, "p", AnimProp::Value, 1.0, 1.0, Easing::Linear));
    assert!(disp.has_anims());
    // 0.5s → 0.5
    assert!(disp.tick_anims(&mut root, 0.5));
    let v = root.base().children[0]
        .animation_value(AnimProp::Value)
        .unwrap();
    assert!((v - 0.5).abs() < 1e-3, "got {v}");
    // 再 0.5s → 1.0 且结束
    disp.tick_anims(&mut root, 0.5);
    assert!(
        (root.base().children[0]
            .animation_value(AnimProp::Value)
            .unwrap()
            - 1.0)
            .abs()
            < 1e-3
    );
    assert!(!disp.has_anims());
    // 结束后无变化
    assert!(!disp.tick_anims(&mut root, 0.5));
}

#[test]
fn frame_animation_only_advances_visible_widgets() {
    let animation = FrameAnimation::new(
        vec![
            flexui_gfx::ImageSource::path("1.png"),
            flexui_gfx::ImageSource::path("2.png"),
        ],
        10.0,
    );
    let style = crate::style::StyleSet::new().with_normal(crate::style::StyleSpec {
        fg_animation: Some(animation),
        ..Default::default()
    });
    let visible = Panel::new()
        .name("visible")
        .size(40.0, 40.0)
        .style(style.clone());
    let hidden = Panel::new().name("hidden").size(40.0, 40.0).style(style);
    let mut root = VBox::new().push(visible).push(hidden);
    root.base_mut().children[1].base_mut().visible = false;
    let mut dispatcher = Dispatcher::new();

    assert!(dispatcher.tick_anims(&mut root, 0.0));
    assert!(root.base().children[0]
        .base()
        .fg_frame_player
        .image()
        .is_some());
    assert!(root.base().children[1]
        .base()
        .fg_frame_player
        .image()
        .is_none());
    assert!(dispatcher.tick_anims(&mut root, 0.11));
    assert!(root.base().children[1]
        .base()
        .fg_frame_player
        .image()
        .is_none());
}

#[test]
fn click_frame_animation_survives_mouse_release_and_finishes() {
    let animation = FrameAnimation::new(
        vec![
            flexui_gfx::ImageSource::path("1.png"),
            flexui_gfx::ImageSource::path("2.png"),
        ],
        10.0,
    )
    .playback(crate::FramePlayback::Once)
    .finish(crate::FrameFinish::Restore);
    let mut root = Button::new("play")
        .name("play")
        .size(100.0, 40.0)
        .click_fg_animation(animation);
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 40.0), &FakeCanvas);
    let mut dispatcher = Dispatcher::new();
    let point = Point::new(50.0, 20.0);
    dispatcher.handle(
        &mut root,
        &Event::MouseDown {
            pos: point,
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    dispatcher.handle(
        &mut root,
        &Event::MouseUp {
            pos: point,
            button: MouseButton::Left,
        },
    );

    assert!(!root.base().pressed);
    assert!(root.base().click_fg_frame_player.image().is_some());
    assert!(dispatcher.tick_anims(&mut root, 0.21));
    assert!(root.base().click_fg_frame_player.image().is_none());
}

#[test]
fn tooltip_延时显示与移开清除() {
    let mut root = VBox::new().push(
        Button::new("b")
            .name("bt")
            .tooltip("提示")
            .size(100.0, 30.0),
    );
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
    let mut disp = Dispatcher::new();
    // 无 hover → 不显示。
    assert!(disp.tooltip_tick(&mut root).is_none());
    // hover 到按钮。
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(50.0, 15.0),
        },
    );
    // 一个 tick 后显示。
    assert!(disp.tooltip_tick(&mut root).is_some());
    assert!(disp.has_tooltip());
    // 已显示：再 tick 不重复。
    assert!(disp.tooltip_tick(&mut root).is_none());
    // hover 移开 → 立即清除。
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(250.0, 250.0),
        },
    );
    assert!(!disp.has_tooltip());
}

#[test]
fn tooltip_无提示文本不显示() {
    let mut root = VBox::new().push(Button::new("b").size(100.0, 30.0));
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(50.0, 15.0),
        },
    );
    assert!(disp.tooltip_tick(&mut root).is_none());
    assert!(!disp.has_tooltip());
}

#[test]
fn edit_剪贴板漏斗() {
    let mut root = Edit::new().name("edit").text("hello");
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    let mut disp = Dispatcher::new();
    // 点击以获得焦点。
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(0.0, 20.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    // 全选 + 复制。
    disp.select_all_focused(&mut root);
    assert_eq!(disp.copy_selection(&mut root).as_deref(), Some("hello"));
    // 剪切 → 返回文本且清空。
    assert_eq!(disp.cut_selection(&mut root).as_deref(), Some("hello"));
    assert_eq!(root.base().text, "");
    assert!(disp.take_layout(), "剪切后必须重建 Edit 字符边界缓存");
    assert!(disp
        .take_control_events()
        .contains(&("edit".to_string(), ControlEvent::TextChanged(String::new()),)));
    // 粘贴。
    disp.paste(&mut root, "hi");
    assert_eq!(root.base().text, "hi");
    assert!(disp.take_layout(), "粘贴后必须重建 Edit 字符边界缓存");
    assert!(disp.take_control_events().contains(&(
        "edit".to_string(),
        ControlEvent::TextChanged("hi".to_string()),
    )));
    // 无选区复制返回 None。
    assert_eq!(disp.copy_selection(&mut root), None);
}

#[test]
fn 子菜单_hover展开且叶项只激活一次() {
    let mut root = Panel::new().size(300.0, 300.0);
    let mut disp = Dispatcher::new();
    let entries = vec![
        crate::widgets::MenuEntry::submenu(
            "提交问题",
            vec![
                crate::widgets::MenuEntry::item("问题百科", "faq"),
                crate::widgets::MenuEntry::item("自助修复", "repair"),
            ],
        ),
        crate::widgets::MenuEntry::item("设置", "settings"),
    ];
    disp.open_styled_menu_entries(
        Rect::new(260.0, 10.0, 24.0, 24.0),
        entries,
        crate::widgets::MenuStyle {
            width: Some(120.0),
            row_height: 40.0,
            alignment: crate::widgets::MenuAlignment::End,
            submenu_align_panel_top: true,
            ..Default::default()
        },
        None,
    );
    disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
    assert_eq!(disp.overlays[0].root.base().rect.right(), 284.0);

    let parent = disp.top_overlay_item_center(0).unwrap();
    disp.handle(&mut root, &Event::MouseMove { pos: parent });
    disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
    assert_eq!(disp.overlays.len(), 2);
    assert_eq!(
        disp.overlays[1].root.base().rect.top(),
        disp.overlays[0].root.base().rect.top()
    );
    assert!(disp.take_activations().is_empty());

    let child = disp.top_overlay_item_center(1).unwrap();
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: child,
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    disp.handle(
        &mut root,
        &Event::MouseUp {
            pos: child,
            button: MouseButton::Left,
        },
    );
    assert_eq!(disp.take_activations(), vec!["repair".to_string()]);
    assert!(!disp.has_overlays());
}

#[test]
fn 窗口失焦关闭全部菜单浮层() {
    let mut root = Button::new("").name("main");
    let mut disp = Dispatcher::new();
    disp.open_menu(
        Rect::new(20.0, 20.0, 20.0, 20.0),
        vec![("设置".into(), "settings".into())],
    );
    assert!(disp.has_overlays());

    disp.handle(&mut root, &Event::WindowFocusChanged { focused: true });
    assert!(disp.has_overlays());

    disp.handle(&mut root, &Event::WindowFocusChanged { focused: false });

    assert!(!disp.has_overlays());
    assert!(disp.take_redraw());
}

#[test]
fn edit_只读剪切不泄露到剪贴板() {
    let mut root = Edit::new().text("secret").read_only(true);
    root.base_mut().rect = Rect::new(0.0, 0.0, 120.0, 30.0);
    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(2.0, 10.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    disp.select_all_focused(&mut root);
    assert_eq!(disp.copy_selection(&mut root).as_deref(), Some("secret"));
    assert_eq!(disp.cut_selection(&mut root), None);
    assert_eq!(root.base().text, "secret");
}

#[test]
fn edit_双击选词() {
    let mut root = Edit::new().text("foo bar");
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    let mut disp = Dispatcher::new();
    // x≈40 落在 "bar" 内 → 选中整词。
    disp.handle(
        &mut root,
        &Event::DoubleClick {
            pos: Point::new(40.0, 20.0),
        },
    );
    assert_eq!(root.selection(), Some((4, 7)));
    assert_eq!(root.selected_text().as_deref(), Some("bar"));
}

#[test]
fn tab_焦点遍历() {
    let mut root = VBox::new()
        .push(Button::new("a").size(50.0, 30.0))
        .push(Button::new("b").size(50.0, 30.0));
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
    let mut disp = Dispatcher::new();
    disp.handle(&mut root, &kd(9));
    assert!(root.base().children[0].base().focused);
    disp.handle(&mut root, &kd(9));
    assert!(root.base().children[1].base().focused);
    assert!(!root.base().children[0].base().focused);
}

#[test]
fn hidden_子树不参与焦点遍历且会清理旧焦点() {
    let mut hidden = Panel::new().push(Edit::new().name("hidden_edit").size(80.0, 30.0));
    hidden.base_mut().visible = false;
    let mut root = VBox::new()
        .push(Button::new("visible").size(80.0, 30.0))
        .push(hidden);
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
    let mut disp = Dispatcher::new();

    disp.handle(&mut root, &kd(9));
    assert!(root.base().children[0].base().focused);

    root.base_mut().children[1].base_mut().visible = true;
    disp.handle(&mut root, &kd(9));
    assert!(root.base().children[1].base().children[0].base().focused);

    root.base_mut().children[1].base_mut().visible = false;
    disp.handle(&mut root, &Event::Char { ch: 'x' });
    assert_eq!(disp.focus(), None);
    assert_eq!(root.base().children[1].base().children[0].base().text, "");
}

#[test]
fn 点击按钮触发回调() {
    let hits = Rc::new(Cell::new(0));
    let h2 = hits.clone();
    let btn = Button::new("ok")
        .size(100.0, 40.0)
        .on_click(move |_ctx| h2.set(h2.get() + 1));
    let mut root = VBox::new().push(btn);
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
    let mut disp = Dispatcher::new();
    click_at(&mut disp, &mut root, Point::new(50.0, 20.0));
    assert_eq!(hits.get(), 1);
}

#[test]
fn 点击回调经_eventctx_可改别的控件() {
    // Button(name=btn) 点击 → 把 Label(name=out) 文本改成 "changed"
    let btn = Button::new("go")
        .name("btn")
        .size(100.0, 40.0)
        .on_click(|ctx| {
            ctx.set_text("out", "changed");
        });
    let out = Label::new("").name("out").size(100.0, 20.0);
    let mut root = VBox::new().push(btn).push(out);
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
    let mut disp = Dispatcher::new();
    // 点击按钮（第一个子控件, y 在 0..40）
    click_at(&mut disp, &mut root, Point::new(50.0, 20.0));
    // 第二个子控件是 out 标签
    assert_eq!(root.base().children[1].base().text, "changed");
}

#[test]
fn hover_设置_hot_状态() {
    let mut root = VBox::new().push(Button::new("b").size(100.0, 40.0));
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(50.0, 20.0),
        },
    );
    assert!(root.base().children[0].base().hover);
    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(150.0, 150.0),
        },
    );
    assert!(!root.base().children[0].base().hover);
}

#[test]
fn hover_脏区包含所有状态阴影() {
    use crate::style::{BaseState, Shadow, StyleSet, StyleSpec};
    use flexui_gfx::Color;

    let style = StyleSet::new()
        .with_normal(StyleSpec::default())
        .with_state(
            BaseState::Hot,
            StyleSpec {
                shadow: Some(Shadow {
                    dx: 8.0,
                    dy: 6.0,
                    color: Color::BLACK,
                }),
                ..Default::default()
            },
        );
    let mut root = VBox::new().push(Button::new("b").size(100.0, 40.0).style(style));
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &FakeCanvas);
    let mut disp = Dispatcher::new();

    disp.handle(
        &mut root,
        &Event::MouseMove {
            pos: Point::new(50.0, 20.0),
        },
    );

    // 按钮固定宽 100 优先于父 Stretch（不再拉伸到 200）；脏区 = 100 + 阴影 dx8 = 108。
    assert_eq!(disp.take_dirty(), Some(Rect::new(0.0, 0.0, 108.0, 46.0)));
}

#[test]
fn radio_同组互斥() {
    let root_v = VBox::new()
        .push(Radio::new("a").group(1).size(100.0, 30.0).selected(true))
        .push(Radio::new("b").group(1).size(100.0, 30.0));
    let mut root = root_v;
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
    let mut disp = Dispatcher::new();
    // 点击第二个 radio（y 在 30~60）
    click_at(&mut disp, &mut root, Point::new(50.0, 45.0));
    assert!(!root.base().children[0].base().selected, "第一个应被取消");
    assert!(root.base().children[1].base().selected, "第二个应选中");
}

#[test]
fn radio_驱动_tabbox_翻页() {
    // 结构：VBox[ Radio(tab0), Radio(tab1), TabBox ]
    let tab = TabBox::new()
        .page(Button::new("page0").size(100.0, 30.0))
        .page(Button::new("page1").size(100.0, 30.0));
    let tab_id = tab.base().id;
    let mut root = VBox::new()
        .push(
            Radio::new("t0")
                .group(9)
                .tab_index(0)
                .size(100.0, 20.0)
                .selected(true),
        )
        .push(Radio::new("t1").group(9).tab_index(1).size(100.0, 20.0))
        .push(tab);
    let cv = FakeCanvas;
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
    let mut disp = Dispatcher::new();
    disp.bind_tab(9, tab_id);
    // 点第二个 radio（y 在 20~40）
    click_at(&mut disp, &mut root, Point::new(50.0, 30.0));
    // TabBox 是第三个子节点
    assert_eq!(root.base().children[2].selected_index(), Some(1));
    assert!(disp.take_layout(), "TabBox 翻页需要重新布局新页面");
}

#[test]
fn eventctx_属性修改自动产生正确失效() {
    let mut root = VBox::new()
        .push(Label::new("old").name("label").size(100.0, 20.0))
        .push(CheckBox::new("check").name("check").size(100.0, 20.0));
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 100.0), &FakeCanvas);

    let mut ctx = EventCtx::new(&mut root);
    assert_eq!(ctx.text("label").as_deref(), Some("old"));
    assert_eq!(
        ctx.take_invalidation(),
        Invalidation::None,
        "只读 getter 不应刷新"
    );

    ctx.set_enabled("check", false);
    assert!(matches!(ctx.take_invalidation(), Invalidation::Paint(_)));

    ctx.set_enabled("check", false);
    assert_eq!(
        ctx.take_invalidation(),
        Invalidation::None,
        "相同值不应重复刷新"
    );

    ctx.set_text("label", "new");
    assert_eq!(ctx.take_invalidation(), Invalidation::Layout);
}

#[test]
fn eventctx_动态子节点操作触发布局() {
    let mut root = Panel::new().name("root");
    let mut ctx = EventCtx::new(&mut root);
    assert!(ctx.add_child("root", Box::new(Label::new("one"))));
    assert_eq!(ctx.take_invalidation(), Invalidation::Layout);
    assert_eq!(ctx.get("root", |w| w.base().children.len()), Some(1));
    assert_eq!(ctx.take_invalidation(), Invalidation::None);
    assert!(ctx.remove_child("root", 0).is_some());
    assert_eq!(ctx.take_invalidation(), Invalidation::Layout);
}

#[test]
fn 纯rust_tabbox_bind_group_驱动翻页() {
    let tab = TabBox::new()
        .bind_group(7)
        .page(Button::new("page0").size(100.0, 30.0))
        .page(Button::new("page1").size(100.0, 30.0));
    let mut root = VBox::new()
        .push(
            Radio::new("t0")
                .group(7)
                .tab_index(0)
                .size(100.0, 20.0)
                .selected(true),
        )
        .push(Radio::new("t1").group(7).tab_index(1).size(100.0, 20.0))
        .push(tab);
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &FakeCanvas);
    let mut disp = Dispatcher::new();

    click_at(&mut disp, &mut root, Point::new(50.0, 30.0));

    assert_eq!(root.base().children[2].selected_index(), Some(1));
    assert!(disp.take_layout(), "纯 Rust TabBox 绑定也需要触发布局");
}

#[test]
fn 控件语义事件同时到达控件回调和窗口队列() {
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = events.clone();
    let mut root = VBox::new().push(
        CheckBox::new("check")
            .name("check")
            .size(100.0, 30.0)
            .on_control_event(move |event, _ctx| captured.borrow_mut().push(event.clone())),
    );
    layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 100.0), &FakeCanvas);
    let mut disp = Dispatcher::new();

    click_at(&mut disp, &mut root, Point::new(50.0, 15.0));

    assert!(events
        .borrow()
        .contains(&ControlEvent::SelectedChanged(true)));
    assert!(disp
        .take_control_events()
        .contains(&("check".to_string(), ControlEvent::SelectedChanged(true),)));
}

#[test]
fn edit_键盘输入发送_text_changed() {
    let mut root = Edit::new().name("edit").size(120.0, 30.0);
    layout_node(&mut root, Rect::new(0.0, 0.0, 120.0, 30.0), &FakeCanvas);
    let mut disp = Dispatcher::new();
    disp.handle(
        &mut root,
        &Event::MouseDown {
            pos: Point::new(10.0, 10.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        },
    );
    disp.take_control_events();

    disp.handle(&mut root, &Event::Char { ch: 'A' });

    assert!(disp.take_control_events().contains(&(
        "edit".to_string(),
        ControlEvent::TextChanged("A".to_string()),
    )));
    assert!(disp.take_layout(), "输入后必须重建 Edit 字符边界缓存");
}

#[test]
fn once帧动画完成发送事件() {
    let animation = FrameAnimation::new(
        vec![
            flexui_gfx::ImageSource::path("1.png"),
            flexui_gfx::ImageSource::path("2.png"),
        ],
        10.0,
    )
    .playback(crate::FramePlayback::Once);
    let mut root = Button::new("run")
        .name("run")
        .size(100.0, 30.0)
        .click_bg_animation(animation);
    layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 30.0), &FakeCanvas);
    let mut disp = Dispatcher::new();
    click_at(&mut disp, &mut root, Point::new(10.0, 10.0));
    disp.take_control_events();

    disp.tick_anims(&mut root, 0.21);

    assert!(disp.take_control_events().contains(&(
        "run".to_string(),
        ControlEvent::FrameAnimationFinished(FrameLayer::Background),
    )));
}

#[test]
fn 专属属性可对称设置读取() {
    let mut root = VBox::new()
        .push(Edit::new().name("edit"))
        .push(ListView::new().name("list"));
    let mut ctx = EventCtx::new(&mut root);
    assert!(ctx.set_property("edit", WidgetProperty::Placeholder("hint".into())));
    assert!(matches!(
        ctx.property("edit", crate::WidgetPropertyKey::Placeholder),
        Some(WidgetProperty::Placeholder(value)) if value == "hint"
    ));
    assert!(ctx.set_property("list", WidgetProperty::Items(vec!["a".into(), "b".into()])));
    assert!(ctx.set_selected_index("list", 1));
    assert_eq!(ctx.selected_index("list"), Some(1));
    assert_eq!(ctx.take_invalidation(), Invalidation::Layout);
}
