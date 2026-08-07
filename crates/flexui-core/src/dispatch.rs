//! 事件分发器（L3）。对应需求 C2/C6/C8/C9。
//!
//! 维护 hover/pressed/focus 状态，做命中测试（含穿透），把鼠标交互翻译为
//! 控件状态变化与点击回调；处理 Radio 分组互斥，并按 tabbar 绑定驱动 TabBox 翻页。

use flexui_geometry::Point;

use crate::event::{Event, EventFlow, MouseButton};
use crate::widget::{HitPolicy, Widget, WidgetId, WidgetRole};

/// Radio 组 → TabBox 的绑定（组成 tabbar）。
pub struct TabBinding {
    pub group: u32,
    pub tabbox: WidgetId,
}

/// 事件分发器。
pub struct Dispatcher {
    hover: Option<WidgetId>,
    pressed: Option<WidgetId>,
    focus: Option<WidgetId>,
    needs_redraw: bool,
    bindings: Vec<TabBinding>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            hover: None,
            pressed: None,
            focus: None,
            needs_redraw: false,
            bindings: Vec::new(),
        }
    }

    /// 注册一个「Radio 组 → TabBox」绑定。
    pub fn bind_tab(&mut self, group: u32, tabbox: WidgetId) {
        self.bindings.push(TabBinding { group, tabbox });
    }

    /// 取走「需要重绘」标志（后端据此调用平台重绘）。
    pub fn take_redraw(&mut self) -> bool {
        let r = self.needs_redraw;
        self.needs_redraw = false;
        r
    }

    /// 当前焦点控件 id。
    pub fn focus(&self) -> Option<WidgetId> {
        self.focus
    }

    /// 分发一个事件到控件树。
    pub fn handle(&mut self, root: &mut dyn Widget, ev: &Event) {
        match ev {
            Event::MouseMove { pos } => {
                let hit = hit_test(root, *pos);
                self.set_hover(root, hit);
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
            } => {
                let hit = hit_test(root, *pos);
                self.press(root, hit);
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
            } => {
                let hit = hit_test(root, *pos);
                self.release(root, hit);
            }
            Event::KeyDown { .. } | Event::KeyUp { .. } | Event::Char { .. } => {
                if let Some(fid) = self.focus {
                    let mut consumed = false;
                    visit_mut(root, fid, &mut |w| {
                        consumed = w.on_event(ev) == EventFlow::Consumed;
                    });
                    if consumed {
                        self.needs_redraw = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// 更新 hover 状态。
    fn set_hover(&mut self, root: &mut dyn Widget, hit: Option<WidgetId>) {
        if self.hover == hit {
            return;
        }
        self.hover = hit;
        for_each_mut(root, &mut |w| {
            let b = w.base_mut();
            b.hover = Some(b.id) == hit && b.enabled;
        });
        self.needs_redraw = true;
    }

    /// 处理按下：设置 pressed 与 focus。
    fn press(&mut self, root: &mut dyn Widget, hit: Option<WidgetId>) {
        self.pressed = hit;

        // 判断命中控件是否可获得焦点。
        let mut focus_target: Option<WidgetId> = None;
        if let Some(id) = hit {
            visit_mut(root, id, &mut |w| {
                let b = w.base();
                if b.focusable && b.enabled {
                    focus_target = Some(b.id);
                }
            });
        }
        self.focus = focus_target;

        for_each_mut(root, &mut |w| {
            let b = w.base_mut();
            b.pressed = Some(b.id) == hit && b.enabled;
            b.focused = Some(b.id) == focus_target;
        });
        self.needs_redraw = true;
    }

    /// 处理抬起：清 pressed；若与按下目标一致则触发点击。
    fn release(&mut self, root: &mut dyn Widget, hit: Option<WidgetId>) {
        let pressed = self.pressed.take();
        for_each_mut(root, &mut |w| w.base_mut().pressed = false);
        self.needs_redraw = true;

        if let Some(pid) = pressed {
            if Some(pid) == hit {
                self.activate(root, pid);
            }
        }
    }

    /// 触发某控件的点击语义：选择切换 + 回调 + Radio 互斥 + tabbar 翻页。
    fn activate(&mut self, root: &mut dyn Widget, id: WidgetId) {
        let mut info: Option<(WidgetRole, Option<u32>, Option<usize>)> = None;
        visit_mut(root, id, &mut |w| {
            let b = w.base_mut();
            if !b.enabled {
                return;
            }
            match b.role {
                WidgetRole::CheckBox => b.selected = !b.selected,
                WidgetRole::Radio => b.selected = true,
                _ => {}
            }
            info = Some((b.role, b.group, b.tab_index));
            // 触发点击回调（取出后调用再放回，避免借用冲突）。
            if let Some(mut cb) = b.on_click.take() {
                cb();
                b.on_click = Some(cb);
            }
        });

        // Radio：同组互斥 + tabbar 联动。
        if let Some((WidgetRole::Radio, Some(g), tab_index)) = info {
            for_each_mut(root, &mut |w| {
                let b = w.base_mut();
                if b.role == WidgetRole::Radio && b.group == Some(g) && b.id != id {
                    b.selected = false;
                }
            });
            if let Some(ti) = tab_index {
                // 找到该组绑定的 TabBox，设置当前页。
                let targets: Vec<WidgetId> = self
                    .bindings
                    .iter()
                    .filter(|bd| bd.group == g)
                    .map(|bd| bd.tabbox)
                    .collect();
                for tb_id in targets {
                    visit_mut(root, tb_id, &mut |w| {
                        w.base_mut().selected_index = ti;
                    });
                }
            }
        }
    }
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// 命中测试：返回最上层「不穿透」且包含该点的可见控件 id。
/// 子控件绘制在父之上，故逆序遍历子控件优先命中。
pub fn hit_test(node: &dyn Widget, p: Point) -> Option<WidgetId> {
    let b = node.base();
    if !b.visible || !b.rect.contains(p) {
        return None;
    }
    for child in b.children.iter().rev() {
        if let Some(id) = hit_test(child.as_ref(), p) {
            return Some(id);
        }
    }
    match b.hit {
        HitPolicy::Solid => Some(b.id),
        HitPolicy::Transparent => None,
    }
}

/// 前序遍历，对每个节点执行 f。
fn for_each_mut(node: &mut dyn Widget, f: &mut dyn FnMut(&mut dyn Widget)) {
    f(node);
    let n = node.base().children.len();
    for i in 0..n {
        for_each_mut(node.base_mut().children[i].as_mut(), f);
    }
}

/// 查找 id 匹配的节点并执行 f，返回是否找到。
fn visit_mut(node: &mut dyn Widget, id: WidgetId, f: &mut dyn FnMut(&mut dyn Widget)) -> bool {
    if node.base().id == id {
        f(node);
        return true;
    }
    let n = node.base().children.len();
    for i in 0..n {
        if visit_mut(node.base_mut().children[i].as_mut(), id, f) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::MouseButton;
    use crate::layout::layout_node;
    use crate::widgets::{Button, Radio, TabBox, VBox};
    use flexui_geometry::{Rect, Size};
    use flexui_gfx::{Canvas, Font};
    use std::cell::Cell;
    use std::rc::Rc;

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

    fn click_at(disp: &mut Dispatcher, root: &mut dyn Widget, p: Point) {
        disp.handle(root, &Event::MouseDown { pos: p, button: MouseButton::Left });
        disp.handle(root, &Event::MouseUp { pos: p, button: MouseButton::Left });
    }

    #[test]
    fn 点击按钮触发回调() {
        let hits = Rc::new(Cell::new(0));
        let h2 = hits.clone();
        let btn = Button::new("ok").size(100.0, 40.0).on_click(move || h2.set(h2.get() + 1));
        let mut root = VBox::new().push(btn);
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
        let mut disp = Dispatcher::new();
        click_at(&mut disp, &mut root, Point::new(50.0, 20.0));
        assert_eq!(hits.get(), 1);
    }

    #[test]
    fn hover_设置_hot_状态() {
        let mut root = VBox::new().push(Button::new("b").size(100.0, 40.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
        let mut disp = Dispatcher::new();
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(50.0, 20.0) });
        assert!(root.base().children[0].base().hover);
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(150.0, 150.0) });
        assert!(!root.base().children[0].base().hover);
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
            .push(Radio::new("t0").group(9).tab_index(0).size(100.0, 20.0).selected(true))
            .push(Radio::new("t1").group(9).tab_index(1).size(100.0, 20.0))
            .push(tab);
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
        let mut disp = Dispatcher::new();
        disp.bind_tab(9, tab_id);
        // 点第二个 radio（y 在 20~40）
        click_at(&mut disp, &mut root, Point::new(50.0, 30.0));
        // TabBox 是第三个子节点
        assert_eq!(root.base().children[2].base().selected_index, 1);
    }
}
