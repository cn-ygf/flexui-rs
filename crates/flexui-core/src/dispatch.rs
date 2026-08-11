//! 事件分发器（L3）。对应需求 C2/C6/C8/C9。
//!
//! 维护 hover/pressed/focus 状态，做命中测试（含穿透），把鼠标交互翻译为
//! 控件状态变化与点击回调；处理 Radio 分组互斥，并按 tabbar 绑定驱动 TabBox 翻页。

use flexui_geometry::{Point, Rect, Size};
use flexui_gfx::Canvas;

use crate::event::{Event, EventFlow, MouseButton};
use crate::layout::layout_node;
use crate::paint::paint_tree;
use crate::widget::{find_by_name, HitPolicy, Node, Widget, WidgetId, WidgetRole};

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

/// 模态浮层（下拉/菜单）：画在最上层、事件独占；选中/点外部/ESC 关闭。
pub struct Overlay {
    /// 浮层内容根（通常是一列 MenuItem 的 VBox）。
    pub root: Node,
    /// 锚点矩形（下拉=触发控件 rect；右键=点位 0 尺寸 rect）。
    pub anchor: Rect,
    /// 归属控件：Some=下拉框（选中回填 owner）；None=右键菜单（按项 name 上报）。
    pub owner: Option<WidgetId>,
    /// 点击浮层外部是否关闭（菜单恒为 true）。
    pub dismiss_outside: bool,
}

/// 被动浮层（Tooltip）：只绘制、从不参与事件；由 hover + 定时器控制显隐。
pub struct Tooltip {
    pub root: Node,
    pub anchor: Rect,
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
    /// 模态浮层栈（下拉/菜单）；顶部为当前活动浮层。
    overlays: Vec<Overlay>,
    /// 浮层内的悬停项（与主树 hover 分离，避免打开菜单清掉主树状态）。
    overlay_hover: Option<WidgetId>,
    /// 浮层内的按下项（与主树 pressed 分离）。
    overlay_pressed: Option<WidgetId>,
    /// 被动 Tooltip 浮层（None=不显示）。
    tooltip: Option<Tooltip>,
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
            overlays: Vec::new(),
            overlay_hover: None,
            overlay_pressed: None,
            tooltip: None,
        }
    }

    /// 是否有活动模态浮层。
    pub fn has_overlays(&self) -> bool {
        !self.overlays.is_empty()
    }

    /// 测试辅助：栈顶浮层第 i 个菜单项的中心点（需先 paint_overlays 布局）。
    #[cfg(test)]
    fn top_overlay_item_center(&self, i: usize) -> Option<Point> {
        let ov = self.overlays.last()?;
        let item = ov.root.base().children.get(i)?;
        let r = item.base().rect;
        Some(Point::new(r.left() + r.size.width / 2.0, r.top() + r.size.height / 2.0))
    }

    /// 测试辅助：栈顶浮层归属 id。
    #[cfg(test)]
    fn top_overlay_owner(&self) -> Option<WidgetId> {
        self.overlays.last().and_then(|o| o.owner)
    }

    /// 绘制所有浮层（模态菜单 + 被动 Tooltip）到最上层（不受主树裁剪）。
    /// 后端在 `paint_tree(root)` 之后调用；`window` 为窗口逻辑尺寸。
    pub fn paint_overlays(&mut self, cv: &mut dyn Canvas, window: Size) {
        for i in 0..self.overlays.len() {
            let anchor = self.overlays[i].anchor;
            let min_w = anchor.size.width; // 菜单至少与锚点同宽
            let node = self.overlays[i].root.as_mut();
            let desired = node.measure(window, &*cv);
            let rect = place_overlay(anchor, desired, window, min_w);
            layout_node(node, rect, &*cv);
            paint_tree(&*node, cv);
        }
        if let Some(tip) = &mut self.tooltip {
            let node = tip.root.as_mut();
            let desired = node.measure(window, &*cv);
            let rect = place_overlay(tip.anchor, desired, window, 0.0);
            layout_node(node, rect, &*cv);
            paint_tree(&*node, cv);
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

    /// 关闭所有模态浮层（并清理浮层内交互态）。
    fn close_overlays(&mut self) {
        self.overlays.clear();
        self.overlay_hover = None;
        self.overlay_pressed = None;
        self.needs_redraw = true;
    }

    /// 有浮层时的事件路由：仅作用于栈顶浮层，不触及主树状态。
    fn handle_with_overlay(&mut self, main_root: &mut dyn Widget, ev: &Event) {
        // 取出栈顶浮层到局部，规避 self 与浮层 root 的自借用冲突。
        let mut ov = match self.overlays.pop() {
            Some(o) => o,
            None => return,
        };
        let mut keep = true;
        match ev {
            Event::MouseMove { pos } => {
                let hit = hit_test(ov.root.as_ref(), *pos);
                self.overlay_hover(ov.root.as_mut(), hit);
            }
            Event::MouseDown { pos, button: MouseButton::Left } => {
                match hit_test(ov.root.as_ref(), *pos) {
                    Some(id) => self.overlay_press(ov.root.as_mut(), Some(id)),
                    None if ov.dismiss_outside => keep = false,
                    None => {}
                }
            }
            Event::MouseUp { pos, button: MouseButton::Left } => {
                let hit = hit_test(ov.root.as_ref(), *pos);
                if self.overlay_release(ov.root.as_mut(), main_root, ov.owner, hit) {
                    keep = false; // 选中即关闭
                }
            }
            Event::KeyDown { key, .. } if *key == crate::event::keys::ESCAPE => {
                keep = false;
            }
            _ => {}
        }
        if keep {
            self.overlays.push(ov);
        } else {
            // ov 已出栈丢弃；清其余浮层与交互态。
            self.close_overlays();
        }
    }

    /// 浮层内 hover（只动浮层树的 hover 标记，主树不受影响）。
    fn overlay_hover(&mut self, ov_root: &mut dyn Widget, hit: Option<WidgetId>) {
        if self.overlay_hover == hit {
            return;
        }
        self.overlay_hover = hit;
        for_each_mut(ov_root, &mut |w| {
            let b = w.base_mut();
            b.hover = Some(b.id) == hit && b.enabled;
        });
        self.needs_redraw = true;
    }

    /// 浮层内按下（记录 overlay_pressed）。
    fn overlay_press(&mut self, ov_root: &mut dyn Widget, hit: Option<WidgetId>) {
        self.overlay_pressed = hit;
        for_each_mut(ov_root, &mut |w| {
            let b = w.base_mut();
            b.pressed = Some(b.id) == hit && b.enabled;
        });
        self.needs_redraw = true;
    }

    /// 浮层内抬起：若按下/抬起同为某 MenuItem，则应用选择并返回 true（应关闭）。
    fn overlay_release(
        &mut self,
        ov_root: &mut dyn Widget,
        main_root: &mut dyn Widget,
        owner: Option<WidgetId>,
        hit: Option<WidgetId>,
    ) -> bool {
        let pressed = self.overlay_pressed.take();
        for_each_mut(ov_root, &mut |w| w.base_mut().pressed = false);
        let Some(pid) = pressed else { return false };
        if Some(pid) != hit {
            return false;
        }
        // 取该 MenuItem 的行号与 name。
        let mut idx: Option<usize> = None;
        let mut item_name: Option<String> = None;
        visit_mut(ov_root, pid, &mut |w| {
            if w.base().role == WidgetRole::MenuItem {
                idx = Some(w.base().selected_index);
                item_name = w.base().name.clone();
            }
        });
        let Some(idx) = idx else { return false };
        match owner {
            // 下拉框：在主树回填 owner 并按 owner 名上报激活。
            Some(oid) => {
                let mut owner_name = None;
                visit_mut(main_root, oid, &mut |w| {
                    w.set_selected_item(idx);
                    owner_name = w.base().name.clone();
                });
                if let Some(n) = owner_name {
                    self.activated.push(n);
                }
            }
            // 右键菜单：按项自身 name 上报激活。
            None => {
                if let Some(n) = item_name {
                    self.activated.push(n);
                }
            }
        }
        true
    }

    /// 复制焦点控件的选中文本（无选区返回 None）。供后端写系统剪贴板。
    pub fn copy_selection(&mut self, root: &mut dyn Widget) -> Option<String> {
        let fid = self.focus?;
        let mut out = None;
        visit_mut(root, fid, &mut |w| out = w.selected_text());
        out
    }

    /// 剪切焦点控件的选中文本：返回文本并删除选区、脏其区域（无选区返回 None）。
    pub fn cut_selection(&mut self, root: &mut dyn Widget) -> Option<String> {
        let fid = self.focus?;
        let mut out = None;
        let mut deleted = false;
        visit_mut(root, fid, &mut |w| {
            out = w.selected_text();
            if out.is_some() {
                deleted = w.delete_selection();
            }
        });
        if deleted {
            if let Some(r) = rect_of(root, fid) {
                self.mark_dirty(r);
            }
        }
        out
    }

    /// 把文本粘贴到焦点控件（替换选区或在光标处插入）。
    pub fn paste(&mut self, root: &mut dyn Widget, s: &str) {
        let Some(fid) = self.focus else { return };
        let mut changed = false;
        visit_mut(root, fid, &mut |w| changed = w.replace_selection(s));
        if changed {
            if let Some(r) = rect_of(root, fid) {
                self.mark_dirty(r);
            }
        }
    }

    /// 全选焦点控件文本。
    pub fn select_all_focused(&mut self, root: &mut dyn Widget) {
        let Some(fid) = self.focus else { return };
        visit_mut(root, fid, &mut |w| w.select_all());
        if let Some(r) = rect_of(root, fid) {
            self.mark_dirty(r);
        }
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
        // 有模态浮层时事件独占给浮层（主树不收事件）。
        if !self.overlays.is_empty() {
            self.handle_with_overlay(root, ev);
            return;
        }
        match ev {
            Event::MouseMove { pos } => {
                let hit = hit_test(root, *pos);
                self.set_hover(root, hit);
                // 若正按住某指针型控件（Edit/Slider）：这是一次拖动 → 转发给它。
                if let Some(id) = self.pressed {
                    if is_pointer_target(role_of(root, id)) {
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
                // 命中指针型控件（Edit/Slider）：转发按下，让其按坐标定位光标/取值。
                if let Some(id) = self.pressed {
                    if is_pointer_target(role_of(root, id)) {
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
        let mut menu: Option<Vec<String>> = None;
        let mut anchor = Rect::default();
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
            anchor = b.rect;
            menu = w.menu_items();
        });
        if !enabled {
            return;
        }
        // 打开下拉的控件（ComboBox）：本次点击只负责弹出菜单，不上报激活
        //（激活留给选中项，避免 on_activate 在打开与选中各触发一次）。
        if let Some(items) = menu {
            let dropdown = crate::widgets::build_menu_labels(&items, Some(id));
            self.overlays.push(Overlay {
                root: dropdown,
                anchor,
                owner: Some(id),
                dismiss_outside: true,
            });
            self.needs_redraw = true;
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

/// 计算浮层摆放矩形：优先锚点下方、放不下则上翻；X 夹到窗内；至少 min_width 宽。
fn place_overlay(anchor: Rect, desired: Size, window: Size, min_width: f32) -> Rect {
    let w = desired.width.max(min_width).min(window.width);
    let h = desired.height.min(window.height);
    // X：锚点左对齐，超出右边则左移，再夹到 0。
    let mut x = anchor.left();
    if x + w > window.width {
        x = window.width - w;
    }
    if x < 0.0 {
        x = 0.0;
    }
    // Y：优先锚点下方；放不下则上翻到上方；再放不下贴底。
    let below = anchor.bottom();
    let y = if below + h <= window.height {
        below
    } else {
        let above = anchor.top() - h;
        if above >= 0.0 {
            above
        } else {
            (window.height - h).max(0.0)
        }
    };
    Rect::new(x, y, w, h)
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

/// 该角色是否接收指针（按下/拖动）事件转发：文本框与滑块。
fn is_pointer_target(role: Option<WidgetRole>) -> bool {
    matches!(role, Some(WidgetRole::Edit) | Some(WidgetRole::Slider))
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
    use crate::widgets::{
        Button, ComboBox, Edit, Label, Panel, Radio, ScrollView, Slider, TabBox, VBox,
    };
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
    fn slider_按下拖动改变值() {
        let mut root = Slider::new().width(100.0).height(20.0);
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 100.0, 20.0), &cv);
        let mut disp = Dispatcher::new();
        // 按下 x=50 → value≈0.5
        disp.handle(
            &mut root,
            &Event::MouseDown { pos: Point::new(50.0, 10.0), button: MouseButton::Left },
        );
        assert!((root.base().value - 0.5).abs() < 1e-3, "got {}", root.base().value);
        // 拖到 x=80 → 0.8
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(80.0, 10.0) });
        assert!((root.base().value - 0.8).abs() < 1e-3);
        // 越界夹取
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(200.0, 10.0) });
        assert_eq!(root.base().value, 1.0);
    }

    #[test]
    fn place_overlay_下方上翻夹取() {
        let win = Size::new(200.0, 100.0);
        // 下方够放：y=锚点底部；宽取 max(desired,min)。
        let r = place_overlay(Rect::new(10.0, 10.0, 50.0, 20.0), Size::new(40.0, 30.0), win, 50.0);
        assert_eq!(r.top(), 30.0);
        assert_eq!(r.size.width, 50.0);
        // 下方放不下 → 上翻到锚点上方。
        let r2 = place_overlay(Rect::new(10.0, 80.0, 50.0, 15.0), Size::new(40.0, 30.0), win, 0.0);
        assert_eq!(r2.top(), 50.0);
        // 锚点靠右 → X 夹到窗内。
        let r3 = place_overlay(Rect::new(180.0, 10.0, 10.0, 10.0), Size::new(40.0, 10.0), win, 0.0);
        assert_eq!(r3.left(), 160.0);
    }

    #[test]
    fn combobox_点击弹下拉_选中回填并上报() {
        let mut root =
            VBox::new().push(ComboBox::new().name("cb").options(["A", "B", "C"]).size(120.0, 30.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
        let mut disp = Dispatcher::new();
        let cb_rect = root.base().children[0].base().rect;
        // 点击 ComboBox → 打开下拉。
        click_at(&mut disp, &mut root, Point::new(cb_rect.left() + 10.0, cb_rect.top() + 10.0));
        assert!(disp.has_overlays());
        assert_eq!(disp.top_overlay_owner(), Some(root.base().children[0].base().id));
        let _ = disp.take_activations(); // 忽略点击 combo 本身的激活
        // 布局浮层后点击第 2 项 "B"。
        disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
        let c = disp.top_overlay_item_center(1).unwrap();
        disp.handle(&mut root, &Event::MouseDown { pos: c, button: MouseButton::Left });
        disp.handle(&mut root, &Event::MouseUp { pos: c, button: MouseButton::Left });
        assert!(!disp.has_overlays(), "选中后关闭");
        assert_eq!(disp.take_activations(), vec!["cb".to_string()]);
        assert_eq!(root.base().children[0].base().text, "B");
    }

    #[test]
    fn 浮层_点外部与esc关闭且不动主树焦点() {
        // 先让一个 Edit 获焦，再打开下拉，验证关闭浮层不清 Edit 焦点。
        let mut root = VBox::new()
            .push(Edit::new().name("e").size(120.0, 30.0))
            .push(ComboBox::new().name("cb").options(["A", "B"]).size(120.0, 30.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
        let mut disp = Dispatcher::new();
        // 聚焦 Edit。
        click_at(&mut disp, &mut root, Point::new(10.0, 15.0));
        let edit_focus = disp.focus();
        assert!(edit_focus.is_some());
        // 打开下拉（点 combo，位于第二行 y≈45）。
        let cb_rect = root.base().children[1].base().rect;
        click_at(&mut disp, &mut root, Point::new(cb_rect.left() + 10.0, cb_rect.top() + 10.0));
        assert!(disp.has_overlays());
        // 点浮层外部 → 关闭；主树焦点不变。
        disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
        disp.handle(&mut root, &Event::MouseDown { pos: Point::new(280.0, 280.0), button: MouseButton::Left });
        assert!(!disp.has_overlays(), "点外部关闭");
        // 再开一次，用 ESC 关。
        click_at(&mut disp, &mut root, Point::new(cb_rect.left() + 10.0, cb_rect.top() + 10.0));
        assert!(disp.has_overlays());
        disp.handle(&mut root, &kd(crate::event::keys::ESCAPE));
        assert!(!disp.has_overlays(), "ESC 关闭");
    }

    #[test]
    fn edit_剪贴板漏斗() {
        let mut root = Edit::new().text("hello");
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
        let mut disp = Dispatcher::new();
        // 点击以获得焦点。
        disp.handle(
            &mut root,
            &Event::MouseDown { pos: Point::new(0.0, 20.0), button: MouseButton::Left },
        );
        // 全选 + 复制。
        disp.select_all_focused(&mut root);
        assert_eq!(disp.copy_selection(&mut root).as_deref(), Some("hello"));
        // 剪切 → 返回文本且清空。
        assert_eq!(disp.cut_selection(&mut root).as_deref(), Some("hello"));
        assert_eq!(root.base().text, "");
        // 粘贴。
        disp.paste(&mut root, "hi");
        assert_eq!(root.base().text, "hi");
        // 无选区复制返回 None。
        assert_eq!(disp.copy_selection(&mut root), None);
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
