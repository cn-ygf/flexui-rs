//! 事件分发器（L3）。对应需求 C2/C6/C8/C9。
//!
//! 维护 hover/pressed/focus 状态，做命中测试（含穿透），把鼠标交互翻译为
//! 控件状态变化与点击回调；处理 Radio 分组互斥，并按 tabbar 绑定驱动 TabBox 翻页。

use flexui_geometry::{Point, Rect};

use crate::event::{Event, EventFlow, MouseButton};
use crate::widget::{find_by_name, HitPolicy, Widget, WidgetId, WidgetRole};

/// 点击回调类型别名。
pub type ClickHandler = Box<dyn FnMut(&mut EventCtx)>;

/// 事件上下文：传给控件回调（on_click），可按 name 访问/修改整棵控件树。
///
/// 因此按钮点击能改别的控件（如更新状态标签），实现「可见的事件响应」。
pub struct EventCtx<'a> {
    root: &'a mut dyn Widget,
}

impl<'a> EventCtx<'a> {
    pub(crate) fn new(root: &'a mut dyn Widget) -> Self {
        Self { root }
    }

    /// 对名为 name 的控件执行 f；找到则返回 Some(f 的返回值)。
    pub fn with<R>(&mut self, name: &str, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        let id = find_by_name(self.root, name)?;
        let mut f = Some(f);
        let mut out = None;
        visit_mut(self.root, id, &mut |w| {
            if let Some(f) = f.take() {
                out = Some(f(w));
            }
        });
        out
    }

    /// 便捷：设置某控件的文本。
    pub fn set_text(&mut self, name: &str, text: impl Into<String>) {
        let text = text.into();
        self.with(name, move |w| w.base_mut().text = text);
    }

    /// 便捷：读取某控件的 selected（CheckBox/Radio）。
    pub fn is_selected(&mut self, name: &str) -> Option<bool> {
        self.with(name, |w| w.base().selected)
    }

    /// 便捷：设置某控件是否可用。
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        self.with(name, move |w| w.base_mut().enabled = enabled);
    }
}

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
    /// 局部脏矩形（联合），后端据此只失效这块区域（脏区重绘）。
    dirty: Option<Rect>,
    bindings: Vec<TabBinding>,
    /// 本轮被激活（点击）的具名控件，供窗口层统一 on_activate 通知（≈ duilib Notify）。
    activated: Vec<String>,
    /// 本轮双击的具名控件。
    double_clicked: Vec<String>,
    /// 本轮右键的具名控件 + 位置（供上下文菜单）。
    context_clicked: Vec<(String, Point)>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            hover: None,
            pressed: None,
            focus: None,
            needs_redraw: false,
            dirty: None,
            bindings: Vec::new(),
            activated: Vec::new(),
            double_clicked: Vec::new(),
            context_clicked: Vec::new(),
        }
    }

    /// 取走本轮被激活的具名控件（窗口驱动据此调 on_activate）。
    pub fn take_activations(&mut self) -> Vec<String> {
        std::mem::take(&mut self.activated)
    }

    /// 取走本轮双击的具名控件。
    pub fn take_double_clicks(&mut self) -> Vec<String> {
        std::mem::take(&mut self.double_clicked)
    }

    /// 取走本轮右键的具名控件 + 位置。
    pub fn take_context_clicks(&mut self) -> Vec<(String, Point)> {
        std::mem::take(&mut self.context_clicked)
    }

    /// 注册一个「Radio 组 → TabBox」绑定。
    pub fn bind_tab(&mut self, group: u32, tabbox: WidgetId) {
        self.bindings.push(TabBinding { group, tabbox });
    }

    /// 取走「需要整窗重绘」标志。
    pub fn take_redraw(&mut self) -> bool {
        let r = self.needs_redraw;
        self.needs_redraw = false;
        r
    }

    /// 取走局部脏矩形（后端优先按此失效；无则看 take_redraw）。
    pub fn take_dirty(&mut self) -> Option<Rect> {
        self.dirty.take()
    }

    /// 累积脏矩形（求并）。
    fn mark_dirty(&mut self, r: Rect) {
        self.dirty = Some(match self.dirty {
            Some(d) => union_rect(d, r),
            None => r,
        });
    }

    /// 光标闪烁：切换焦点控件 caret 相位，返回其矩形供局部失效（无焦点返回 None）。
    pub fn blink(&mut self, root: &mut dyn Widget) -> Option<Rect> {
        let fid = self.focus?;
        let mut rect = None;
        visit_mut(root, fid, &mut |w| {
            let b = w.base_mut();
            b.caret_on = !b.caret_on;
            rect = Some(b.rect);
        });
        rect
    }

    /// 当前焦点控件 id。
    pub fn focus(&self) -> Option<WidgetId> {
        self.focus
    }

    /// 把事件转发给指定 id 的控件 on_event；消费则脏其区域。
    fn forward_to_widget(&mut self, root: &mut dyn Widget, id: WidgetId, ev: &Event) {
        let mut consumed = false;
        visit_mut(root, id, &mut |w| {
            consumed = w.on_event(ev) == EventFlow::Consumed;
        });
        if consumed {
            if let Some(r) = rect_of(root, id) {
                self.mark_dirty(r);
            }
        }
    }

    /// 分发一个事件到控件树。
    pub fn handle(&mut self, root: &mut dyn Widget, ev: &Event) {
        match ev {
            Event::MouseMove { pos } => {
                let hit = hit_test(root, *pos);
                self.set_hover(root, hit);
                // 若正按住某文本控件：这是一次拖动 → 转发给它延伸选区。
                if let Some(id) = self.pressed {
                    if role_of(root, id) == Some(WidgetRole::Edit) {
                        self.forward_to_widget(root, id, ev);
                    }
                }
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
            } => {
                let hit = hit_test(root, *pos);
                self.press(root, hit);
                // 命中文本控件：转发按下事件，让其按 x 定位光标/起始锚点。
                if let Some(id) = self.focus {
                    if role_of(root, id) == Some(WidgetRole::Edit) {
                        self.forward_to_widget(root, id, ev);
                    }
                }
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
            } => {
                let hit = hit_test(root, *pos);
                self.release(root, hit);
            }
            // 右键抬起 → 记录具名控件供上下文菜单（on_context）。
            Event::MouseUp {
                pos,
                button: MouseButton::Right,
            } => {
                if let Some(id) = hit_test(root, *pos) {
                    if let Some(name) = name_of(root, id) {
                        self.context_clicked.push((name, *pos));
                    }
                }
            }
            // 双击 → 记录具名控件（on_double_click）；命中文本控件则转发做选词。
            Event::DoubleClick { pos } => {
                if let Some(id) = hit_test(root, *pos) {
                    if let Some(name) = name_of(root, id) {
                        self.double_clicked.push(name);
                    }
                    if role_of(root, id) == Some(WidgetRole::Edit) {
                        self.forward_to_widget(root, id, ev);
                    }
                }
            }
            // Tab 键：焦点在可聚焦控件间遍历。
            Event::KeyDown { key: 9, .. } => {
                self.focus_next(root);
            }
            Event::KeyDown { .. } | Event::KeyUp { .. } | Event::Char { .. } => {
                if let Some(fid) = self.focus {
                    let mut consumed = false;
                    visit_mut(root, fid, &mut |w| {
                        consumed = w.on_event(ev) == EventFlow::Consumed;
                    });
                    if consumed {
                        // 只脏焦点控件区域（打字只重绘输入框）。
                        if let Some(r) = rect_of(root, fid) {
                            self.mark_dirty(r);
                        }
                    }
                }
            }
            // 滚轮：滚动光标下最内层可滚动容器。
            Event::MouseWheel { pos, dy, .. } => {
                self.scroll_at(root, *pos, *dy);
            }
            _ => {}
        }
    }

    /// 焦点移到下一个可聚焦控件（Tab 遍历）。
    fn focus_next(&mut self, root: &mut dyn Widget) {
        let mut order: Vec<WidgetId> = Vec::new();
        for_each_mut(root, &mut |w| {
            let b = w.base();
            if b.focusable && b.enabled && b.visible {
                order.push(b.id);
            }
        });
        if order.is_empty() {
            return;
        }
        let next = match self.focus.and_then(|f| order.iter().position(|&id| id == f)) {
            Some(i) => order[(i + 1) % order.len()],
            None => order[0],
        };
        self.focus = Some(next);
        for_each_mut(root, &mut |w| {
            let b = w.base_mut();
            b.focused = b.id == next;
            if b.focused {
                b.caret_on = true;
            }
        });
        self.needs_redraw = true;
    }

    /// 滚动光标下最内层可滚动容器 dy 像素（正 dy=内容上滚）。
    fn scroll_at(&mut self, root: &mut dyn Widget, pos: Point, dy: f32) {
        // 找到包含该点的最深可滚动容器 id。
        let mut target: Option<WidgetId> = None;
        for_each_mut(root, &mut |w| {
            let b = w.base();
            if b.scrollable && b.visible && b.rect.contains(pos) {
                target = Some(b.id); // 后序覆盖 → 保留最深（子在父后遍历）
            }
        });
        if let Some(id) = target {
            let mut rect = None;
            visit_mut(root, id, &mut |w| {
                let b = w.base_mut();
                let max_scroll = (b.content_h - b.rect.size.height).max(0.0);
                b.scroll_y = (b.scroll_y - dy).clamp(0.0, max_scroll);
                rect = Some(b.rect);
            });
            if let Some(r) = rect {
                self.mark_dirty(r); // 只脏滚动区
            }
        }
    }

    /// 更新 hover 状态（只脏「旧+新」悬停控件区域）。
    fn set_hover(&mut self, root: &mut dyn Widget, hit: Option<WidgetId>) {
        if self.hover == hit {
            return;
        }
        let old = self.hover;
        self.hover = hit;
        for_each_mut(root, &mut |w| {
            let b = w.base_mut();
            b.hover = Some(b.id) == hit && b.enabled;
        });
        for id in [old, hit].into_iter().flatten() {
            if let Some(r) = rect_of(root, id) {
                self.mark_dirty(r);
            }
        }
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
            if b.focused {
                b.caret_on = true; // 获焦立即显示光标
            }
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

    /// 触发某控件的点击语义：选择切换 + Radio 互斥 + tabbar 翻页 + 回调。
    fn activate(&mut self, root: &mut dyn Widget, id: WidgetId) {
        // 1. 选择态变化 + 采集信息（此时不触发回调）。
        let mut info: Option<(WidgetRole, Option<u32>, Option<usize>)> = None;
        let mut enabled = false;
        let mut activated_name: Option<String> = None;
        visit_mut(root, id, &mut |w| {
            let b = w.base_mut();
            if !b.enabled {
                return;
            }
            enabled = true;
            match b.role {
                WidgetRole::CheckBox => b.selected = !b.selected,
                WidgetRole::Radio => b.selected = true,
                _ => {}
            }
            info = Some((b.role, b.group, b.tab_index));
            activated_name = b.name.clone();
        });
        if !enabled {
            return;
        }
        // 记录具名激活控件，供窗口层 on_activate 统一通知。
        if let Some(name) = activated_name {
            self.activated.push(name);
        }

        // 2. Radio：同组互斥 + tabbar 联动。
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

        // 3. 触发点击回调：先把闭包取出（释放对该节点的借用），再以 EventCtx 调用
        //    （此时回调可 &mut 整棵树、按 name 改别的控件），最后放回。
        let mut taken: Option<ClickHandler> = None;
        visit_mut(root, id, &mut |w| {
            taken = w.base_mut().on_click.take();
        });
        if let Some(mut cb) = taken {
            let mut ctx = EventCtx::new(root);
            cb(&mut ctx);
            let mut slot = Some(cb);
            visit_mut(root, id, &mut |w| {
                w.base_mut().on_click = slot.take();
            });
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

/// 两矩形的包围并集。
fn union_rect(a: Rect, b: Rect) -> Rect {
    let l = a.left().min(b.left());
    let t = a.top().min(b.top());
    let r = a.right().max(b.right());
    let bo = a.bottom().max(b.bottom());
    Rect::new(l, t, r - l, bo - t)
}

/// 按 id 找控件的 name。
fn name_of(node: &dyn Widget, id: WidgetId) -> Option<String> {
    if node.base().id == id {
        return node.base().name.clone();
    }
    for child in node.base().children.iter() {
        if let Some(n) = name_of(child.as_ref(), id) {
            return Some(n);
        }
    }
    None
}

/// 按 id 找控件的角色（用于判断是否文本控件）。
fn role_of(node: &dyn Widget, id: WidgetId) -> Option<WidgetRole> {
    if node.base().id == id {
        return Some(node.base().role);
    }
    for child in node.base().children.iter() {
        if let Some(r) = role_of(child.as_ref(), id) {
            return Some(r);
        }
    }
    None
}

/// 按 id 找控件的绝对矩形。
fn rect_of(node: &dyn Widget, id: WidgetId) -> Option<Rect> {
    if node.base().id == id {
        return Some(node.base().rect);
    }
    for child in node.base().children.iter() {
        if let Some(r) = rect_of(child.as_ref(), id) {
            return Some(r);
        }
    }
    None
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
    use crate::event::{Mods, MouseButton};
    use crate::layout::layout_node;
    use crate::widgets::{Button, Edit, Label, Panel, Radio, ScrollView, TabBox, VBox};
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

    /// 构造无修饰键的 KeyDown（测试便捷）。
    fn kd(key: u32) -> Event {
        Event::KeyDown { key, mods: Mods::default() }
    }

    fn click_at(disp: &mut Dispatcher, root: &mut dyn Widget, p: Point) {
        disp.handle(root, &Event::MouseDown { pos: p, button: MouseButton::Left });
        disp.handle(root, &Event::MouseUp { pos: p, button: MouseButton::Left });
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
        assert_eq!(root.base().content_h, 200.0);
        assert_eq!(root.base().children[0].base().rect.top(), 0.0);

        let mut disp = Dispatcher::new();
        disp.handle(
            &mut root,
            &Event::MouseWheel { pos: Point::new(50.0, 50.0), dx: 0.0, dy: -60.0 },
        );
        assert_eq!(root.base().scroll_y, 60.0); // 视口100 内容200 → 可滚到 100，60 有效
        // 重新布局后首个子应上移 60
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
        assert_eq!(root.base().children[0].base().rect.top(), -60.0);
    }

    #[test]
    fn 双击与右键记录() {
        let mut root = VBox::new().push(Button::new("x").name("b").size(50.0, 30.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &cv);
        let mut disp = Dispatcher::new();
        disp.handle(&mut root, &Event::DoubleClick { pos: Point::new(25.0, 15.0) });
        assert_eq!(disp.take_double_clicks(), vec!["b".to_string()]);
        disp.handle(
            &mut root,
            &Event::MouseUp { pos: Point::new(25.0, 15.0), button: MouseButton::Right },
        );
        let ctx = disp.take_context_clicks();
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx[0].0, "b");
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
            &Event::MouseDown { pos: Point::new(25.0, 20.0), button: MouseButton::Left },
        );
        assert_eq!(root.base().cursor, 3);
        assert_eq!(root.base().sel_range(), None);
        // 拖到 x≈8 → 边界1；选区 (1,3) = "el"。
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(8.0, 20.0) });
        assert_eq!(root.base().sel_range(), Some((1, 3)));
        assert_eq!(root.selected_text().as_deref(), Some("el"));
    }

    #[test]
    fn edit_双击选词() {
        let mut root = Edit::new().text("foo bar");
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        let mut disp = Dispatcher::new();
        // x≈40 落在 "bar" 内 → 选中整词。
        disp.handle(&mut root, &Event::DoubleClick { pos: Point::new(40.0, 20.0) });
        assert_eq!(root.base().sel_range(), Some((4, 7)));
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
    fn 点击按钮触发回调() {
        let hits = Rc::new(Cell::new(0));
        let h2 = hits.clone();
        let btn = Button::new("ok").size(100.0, 40.0).on_click(move |_ctx| h2.set(h2.get() + 1));
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
        let btn = Button::new("go").name("btn").size(100.0, 40.0).on_click(|ctx| {
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
