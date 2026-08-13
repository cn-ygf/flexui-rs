//! 事件分发器（L3）。对应需求 C2/C6/C8/C9。
//!
//! 维护 hover/pressed/focus 状态，做命中测试（含穿透），把鼠标交互翻译为
//! 控件状态变化与点击回调；处理 Radio 分组互斥，并按 tabbar 绑定驱动 TabBox 翻页。

use std::sync::{Arc, Mutex};

use flexui_geometry::{Point, Rect, Size};
use flexui_gfx::Canvas;

use crate::anim::{Anim, AnimProp, Easing};
use crate::event::{Event, EventFlow, MouseButton};
use crate::layout::layout_node;
use crate::paint::paint_tree;
use crate::widget::{find_by_name, HitPolicy, Node, Widget, WidgetId, WidgetRole};

/// 点击回调类型别名。
pub type ClickHandler = Box<dyn FnMut(&mut EventCtx)>;

/// 后台线程 → 主线程投递句柄（Clone + Send + Sync）。
///
/// 工作线程持有其克隆，`send` 把字符串消息投递到主线程邮箱；主线程（帧定时回调）
/// 取走并交窗口委托 `on_message`。
#[derive(Clone)]
pub struct MainProxy {
    queue: Arc<Mutex<Vec<String>>>,
}

impl MainProxy {
    /// 投递一条消息到主线程（跨线程安全）。
    pub fn send(&self, msg: impl Into<String>) {
        if let Ok(mut q) = self.queue.lock() {
            q.push(msg.into());
        }
    }
}

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
        self.with(name, move |w| {
            w.base_mut().localizations.retain(|binding| !matches!(binding, crate::LocalizationBinding::Text(_)));
            w.set_text_value(text);
        });
    }

    /// 便捷：读取某控件的 selected（CheckBox/Radio）。
    pub fn is_selected(&mut self, name: &str) -> Option<bool> {
        self.with(name, |w| w.base().selected)
    }

    /// 便捷：设置某控件是否可用。
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        self.with(name, move |w| w.base_mut().enabled = enabled);
    }

    /// 便捷：设置某控件及其子树是否参与布局、绘制与命中测试。
    pub fn set_visible(&mut self, name: &str, visible: bool) {
        self.with(name, move |w| w.base_mut().visible = visible);
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
    /// 浮层相对窗口边缘的安全边距。
    pub window_margin: flexui_geometry::Insets,
    /// 当前层的菜单数据，用于按行展开子菜单。
    pub entries: Vec<crate::widgets::MenuEntry>,
    pub style: crate::widgets::MenuStyle,
    /// 子菜单所属的父菜单项；根菜单为 None。
    pub parent_item: Option<WidgetId>,
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
    overlay_hover: Option<(usize, WidgetId)>,
    /// 浮层内的按下项（与主树 pressed 分离）。
    overlay_pressed: Option<(usize, WidgetId)>,
    /// 被动 Tooltip 浮层（None=不显示）。
    tooltip: Option<Tooltip>,
    /// hover 停留在带 tooltip 控件上累计的定时 tick 数（用于延时显示）。
    tip_ticks: u32,
    /// 进行中的补间动画。
    anims: Vec<Anim>,
    /// 后台线程投递的消息邮箱（主线程帧回调取走）。
    mailbox: Arc<Mutex<Vec<String>>>,
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
            tip_ticks: 0,
            anims: Vec::new(),
            mailbox: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 取一个可跨线程投递消息的句柄（发给工作线程）。
    pub fn proxy(&self) -> MainProxy {
        MainProxy { queue: Arc::clone(&self.mailbox) }
    }

    /// 取走后台线程投递的所有消息（主线程调用）。
    pub fn drain_messages(&self) -> Vec<String> {
        self.mailbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }

    /// 是否有进行中的动画（后端据此决定是否驱动帧定时器）。
    pub fn has_anims(&self) -> bool {
        !self.anims.is_empty()
    }

    /// 启动一条属性补间动画：把名为 name 的控件的 prop 从当前值过渡到 to。
    /// 同一控件同一属性的旧动画会被替换。返回是否找到该控件。
    pub fn animate(
        &mut self,
        root: &mut dyn Widget,
        name: &str,
        prop: AnimProp,
        to: f32,
        dur_secs: f32,
        easing: Easing,
    ) -> bool {
        let Some(id) = find_by_name(root, name) else { return false };
        let mut from = None;
        visit_mut(root, id, &mut |w| from = w.animation_value(prop));
        let Some(from) = from else { return false };
        self.anims.retain(|a| !(a.target == id && a.prop == prop));
        self.anims.push(Anim {
            target: id,
            prop,
            from,
            to,
            dur: dur_secs.max(0.001),
            elapsed: 0.0,
            easing,
        });
        true
    }

    /// 按帧推进所有动画 dt 秒；应用插值到目标控件，移除结束的。返回是否有变化（需重绘）。
    pub fn tick_anims(&mut self, root: &mut dyn Widget, dt: f32) -> bool {
        if self.anims.is_empty() {
            return false;
        }
        for a in &mut self.anims {
            a.elapsed += dt;
        }
        // 快照 (target, prop, value) 后应用，避免与 self 的可变借用冲突。
        let apply: Vec<(WidgetId, AnimProp, f32)> =
            self.anims.iter().map(|a| (a.target, a.prop, a.value_at())).collect();
        for (id, prop, v) in apply {
            visit_mut(root, id, &mut |w| { w.set_animation_value(prop, v); });
        }
        self.anims.retain(|a| !a.done());
        self.needs_redraw = true;
        true
    }

    /// 是否有活动模态浮层。
    pub fn has_overlays(&self) -> bool {
        !self.overlays.is_empty()
    }

    /// 关闭当前窗口的全部菜单浮层。
    pub fn dismiss_overlays(&mut self) {
        if !self.overlays.is_empty() {
            self.close_overlays();
        }
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

    /// 测试辅助：是否正显示 Tooltip。
    #[cfg(test)]
    fn has_tooltip(&self) -> bool {
        self.tooltip.is_some()
    }

    /// 打开一个上下文菜单（owner=None，选中项按其 name 上报激活）。
    pub fn open_menu(&mut self, anchor: Rect, items: Vec<(String, String)>) {
        self.open_styled_menu(anchor, items, None, None);
    }

    /// 打开可定制外观并标记当前项的上下文菜单。
    pub fn open_styled_menu(
        &mut self,
        anchor: Rect,
        items: Vec<(String, String)>,
        style: Option<crate::widgets::MenuStyle>,
        selected_name: Option<String>,
    ) {
        let style = style.unwrap_or_default();
        let entries = items
            .into_iter()
            .map(|(label, name)| crate::widgets::MenuEntry::item(label, name))
            .collect();
        self.open_styled_menu_entries(anchor, entries, style, selected_name);
    }

    /// 打开可带图标与子菜单的上下文菜单。
    pub fn open_styled_menu_entries(
        &mut self,
        anchor: Rect,
        entries: Vec<crate::widgets::MenuEntry>,
        style: crate::widgets::MenuStyle,
        selected_name: Option<String>,
    ) {
        if entries.is_empty() {
            return;
        }
        let menu = crate::widgets::build_menu_entries(&entries, &style, selected_name.as_deref());
        self.overlays.push(Overlay {
            root: menu,
            anchor,
            owner: None,
            dismiss_outside: true,
            window_margin: style.window_margin,
            entries,
            style,
            parent_item: None,
        });
        self.needs_redraw = true;
    }

    /// 绘制所有浮层（模态菜单 + 被动 Tooltip）到最上层（不受主树裁剪）。
    /// 后端在 `paint_tree(root)` 之后调用；`window` 为窗口逻辑尺寸。
    pub fn paint_overlays(&mut self, cv: &mut dyn Canvas, window: Size) {
        for i in 0..self.overlays.len() {
            let anchor = self.overlays[i].anchor;
            let window_margin = self.overlays[i].window_margin;
            let is_submenu = self.overlays[i].parent_item.is_some();
            let alignment = self.overlays[i].style.alignment;
            let offset = self.overlays[i].style.offset;
            let min_w = anchor.size.width; // 菜单至少与锚点同宽
            let node = self.overlays[i].root.as_mut();
            let desired = node.measure(window, &*cv);
            let mut rect = if is_submenu {
                place_submenu(anchor, desired, window, window_margin)
            } else {
                place_overlay(
                    anchor,
                    desired,
                    window,
                    min_w,
                    window_margin,
                    alignment,
                )
            };
            rect.origin.x += offset.x;
            rect.origin.y += offset.y;
            layout_node(node, rect, &*cv);
            paint_tree(&*node, cv);
        }
        if let Some(tip) = &mut self.tooltip {
            let node = tip.root.as_mut();
            let desired = node.measure(window, &*cv);
            let rect = place_overlay(
                tip.anchor,
                desired,
                window,
                0.0,
                flexui_geometry::Insets::default(),
                crate::widgets::MenuAlignment::Start,
            );
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
        self.ensure_focus_valid(root);
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

    /// 有浮层时的事件路由：从最上层向下命中，父子菜单作为一个整体交互。
    fn handle_with_overlay(&mut self, main_root: &mut dyn Widget, ev: &Event) {
        match ev {
            Event::MouseMove { pos } => {
                let Some(level) = self.overlay_level_at(*pos) else {
                    self.clear_overlay_hover();
                    return;
                };
                self.overlays.truncate(level + 1);
                let hit = hit_test(self.overlays[level].root.as_ref(), *pos);
                self.overlay_hover(level, hit);
                self.open_hovered_submenu(level, hit);
            }
            Event::MouseDown { pos, button: MouseButton::Left } => {
                if let Some(level) = self.overlay_level_at(*pos) {
                    let hit = hit_test(self.overlays[level].root.as_ref(), *pos);
                    self.overlay_press(level, hit);
                } else {
                    self.close_overlays();
                }
            }
            Event::MouseUp { pos, button: MouseButton::Left } => {
                if let Some(level) = self.overlay_level_at(*pos) {
                    let hit = hit_test(self.overlays[level].root.as_ref(), *pos);
                    if self.overlay_release(level, main_root, hit) {
                        self.close_overlays();
                    }
                }
            }
            Event::KeyDown { key, .. } if *key == crate::event::keys::ESCAPE => {
                self.overlays.pop();
                self.overlay_hover = None;
                self.overlay_pressed = None;
                self.needs_redraw = true;
            }
            Event::MouseWheel { pos, dy, .. } => {
                if let Some(level) = self.overlay_level_at(*pos) {
                    if scroll_tree_at(self.overlays[level].root.as_mut(), *pos, *dy).is_some() {
                        self.overlays.truncate(level + 1);
                        self.needs_redraw = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// 浮层内 hover（只动浮层树的 hover 标记，主树不受影响）。
    fn overlay_hover(&mut self, level: usize, hit: Option<WidgetId>) {
        let hover = hit.map(|id| (level, id));
        if self.overlay_hover == hover {
            return;
        }
        self.overlay_hover = hover;
        for (overlay_level, overlay) in self.overlays.iter_mut().enumerate() {
            for_each_mut(overlay.root.as_mut(), &mut |w| {
                let b = w.base_mut();
                b.hover = hover == Some((overlay_level, b.id)) && b.enabled;
            });
        }
        self.needs_redraw = true;
    }

    fn clear_overlay_hover(&mut self) {
        if self.overlay_hover.take().is_none() {
            return;
        }
        for overlay in &mut self.overlays {
            for_each_mut(overlay.root.as_mut(), &mut |w| w.base_mut().hover = false);
        }
        self.needs_redraw = true;
    }

    /// 浮层内按下（记录 overlay_pressed）。
    fn overlay_press(&mut self, level: usize, hit: Option<WidgetId>) {
        self.overlay_pressed = hit.map(|id| (level, id));
        for (overlay_level, overlay) in self.overlays.iter_mut().enumerate() {
            for_each_mut(overlay.root.as_mut(), &mut |w| {
                let b = w.base_mut();
                b.pressed = self.overlay_pressed == Some((overlay_level, b.id)) && b.enabled;
            });
        }
        self.needs_redraw = true;
    }

    /// 浮层内抬起：若按下/抬起同为某 MenuItem，则应用选择并返回 true（应关闭）。
    fn overlay_release(
        &mut self,
        level: usize,
        main_root: &mut dyn Widget,
        hit: Option<WidgetId>,
    ) -> bool {
        let pressed = self.overlay_pressed.take();
        for overlay in &mut self.overlays {
            for_each_mut(overlay.root.as_mut(), &mut |w| w.base_mut().pressed = false);
        }
        let Some((pressed_level, pid)) = pressed else { return false };
        if pressed_level != level || Some(pid) != hit {
            return false;
        }
        // 取该 MenuItem 的行号与 name。
        let mut idx: Option<usize> = None;
        let mut item_name: Option<String> = None;
        visit_mut(self.overlays[level].root.as_mut(), pid, &mut |w| {
            if w.base().role == WidgetRole::MenuItem {
                idx = w.selected_index();
                item_name = w.base().name.clone();
            }
        });
        let Some(idx) = idx else { return false };
        if self.overlays[level].entries.get(idx).is_some_and(crate::widgets::MenuEntry::is_submenu) {
            self.open_hovered_submenu(level, Some(pid));
            return false;
        }
        let owner = self.overlays[level].owner;
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

    fn overlay_level_at(&self, pos: Point) -> Option<usize> {
        self.overlays.iter().rposition(|overlay| overlay.root.base().rect.contains(pos))
    }

    fn open_hovered_submenu(&mut self, level: usize, hit: Option<WidgetId>) {
        let Some(item_id) = hit else {
            self.overlays.truncate(level + 1);
            return;
        };
        let mut index = None;
        let mut anchor = Rect::default();
        visit_mut(self.overlays[level].root.as_mut(), item_id, &mut |widget| {
            if widget.base().role == WidgetRole::MenuItem {
                index = widget.selected_index();
                anchor = widget.base().rect;
            }
        });
        let Some(entry) = index.and_then(|index| self.overlays[level].entries.get(index)).cloned() else {
            self.overlays.truncate(level + 1);
            return;
        };
        if !entry.is_submenu() {
            self.overlays.truncate(level + 1);
            return;
        }
        if self.overlays.get(level + 1).is_some_and(|overlay| overlay.parent_item == Some(item_id)) {
            return;
        }
        self.overlays.truncate(level + 1);
        if self.overlays[level].style.submenu_align_panel_top {
            anchor.origin.y = self.overlays[level].root.base().rect.top();
        }
        let style = self.overlays[level]
            .style
            .submenu_style
            .as_deref()
            .unwrap_or(&self.overlays[level].style)
            .clone();
        let root = crate::widgets::build_menu_entries(&entry.children, &style, None);
        self.overlays.push(Overlay {
            root,
            anchor,
            owner: None,
            dismiss_outside: true,
            window_margin: style.window_margin,
            entries: entry.children,
            style,
            parent_item: Some(item_id),
        });
        self.needs_redraw = true;
    }

    /// 复制焦点控件的选中文本（无选区返回 None）。供后端写系统剪贴板。
    pub fn copy_selection(&mut self, root: &mut dyn Widget) -> Option<String> {
        self.ensure_focus_valid(root);
        let fid = self.focus?;
        let mut out = None;
        visit_mut(root, fid, &mut |w| out = w.selected_text());
        out
    }

    /// 剪切焦点控件的选中文本：返回文本并删除选区、脏其区域（无选区返回 None）。
    pub fn cut_selection(&mut self, root: &mut dyn Widget) -> Option<String> {
        self.ensure_focus_valid(root);
        let fid = self.focus?;
        let mut out = None;
        let mut deleted = false;
        visit_mut(root, fid, &mut |w| {
            out = w.selected_text();
            if out.is_some() {
                deleted = w.delete_selection();
                if !deleted {
                    out = None;
                }
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
        self.ensure_focus_valid(root);
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
        self.ensure_focus_valid(root);
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
        self.ensure_focus_valid(root);
        if matches!(ev, Event::WindowFocusChanged { focused: false }) {
            self.dismiss_overlays();
            return;
        }
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
                let old_focus = self.focus;
                self.press(root, hit);
                // 命中指针型控件（Edit/Slider/ListView）：转发按下，让其按坐标定位/选中。
                if let Some(id) = self.pressed {
                    let role = role_of(root, id);
                    if is_pointer_target(role) {
                        self.forward_to_widget(root, id, ev);
                        if old_focus != self.focus && role == Some(WidgetRole::Edit) {
                            visit_mut(root, id, &mut |w| w.focus_gained());
                        }
                        // 列表点击即选择：按 name 上报，供窗口层 on_activate 处理。
                        if role == Some(WidgetRole::ListView) {
                            if let Some(name) = name_of(root, id) {
                                self.activated.push(name);
                            }
                        }
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
        for_each_visible_mut(root, true, &mut |w| {
            let b = w.base();
            if b.focusable && b.enabled {
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
            let focused = w.base().id == next;
            w.base_mut().focused = focused;
            if focused {
                w.base_mut().caret_on = true;
                w.focus_gained();
            }
        });
        self.needs_redraw = true;
    }

    /// 隐藏或禁用当前焦点控件后，立即停止向它分发键盘与剪贴板事件。
    fn ensure_focus_valid(&mut self, root: &mut dyn Widget) {
        let Some(id) = self.focus else { return };
        if is_focus_candidate(root, id, true) {
            return;
        }
        self.focus = None;
        for_each_mut(root, &mut |w| w.base_mut().focused = false);
        self.needs_redraw = true;
    }

    /// 滚动光标下最内层可滚动容器 dy 像素（正 dy=内容上滚）。
    fn scroll_at(&mut self, root: &mut dyn Widget, pos: Point, dy: f32) {
        if let Some(rect) = scroll_tree_at(root, pos, dy) {
            self.mark_dirty(rect); // 只脏滚动区
        }
    }

    /// 更新 hover 状态（只脏「旧+新」悬停控件区域）。
    fn set_hover(&mut self, root: &mut dyn Widget, hit: Option<WidgetId>) {
        if self.hover == hit {
            return;
        }
        let old = self.hover;
        self.hover = hit;
        // hover 变化：重置 tooltip 计时并立即隐藏当前提示。
        self.tip_ticks = 0;
        if self.tooltip.take().is_some() {
            self.needs_redraw = true;
        }
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

    /// 定时器 tick 驱动 Tooltip 延时显示：hover 停留在带 tooltip 的控件累计到阈值即显示。
    /// 返回需失效的矩形（供后端重绘）。由后端在闪烁定时回调里调用。
    pub fn tooltip_tick(&mut self, root: &mut dyn Widget) -> Option<Rect> {
        // 菜单打开或已显示提示时不处理。
        if !self.overlays.is_empty() || self.tooltip.is_some() {
            return None;
        }
        let Some(id) = self.hover else {
            self.tip_ticks = 0;
            return None;
        };
        // 读悬停控件的 tooltip 文本与矩形。
        let mut text: Option<String> = None;
        let mut rect = Rect::default();
        visit_mut(root, id, &mut |w| {
            text = w.base().tooltip.clone();
            rect = w.base().rect;
        });
        let Some(text) = text.filter(|s| !s.is_empty()) else {
            self.tip_ticks = 0;
            return None;
        };
        self.tip_ticks += 1;
        // 累计满 1 个 tick（≈0.53s）后显示。
        if self.tip_ticks >= 1 {
            self.tooltip = Some(Tooltip { root: crate::widgets::build_tooltip(&text), anchor: rect });
            self.needs_redraw = true;
            return Some(rect);
        }
        None
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
            let group = w.selection_group();
            let tab_index = w.tab_index();
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
            info = Some((b.role, group, tab_index));
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
            let entries = items
                .iter()
                .map(|label| crate::widgets::MenuEntry::item(label.clone(), ""))
                .collect();
            let style = crate::widgets::MenuStyle::default();
            let dropdown = crate::widgets::build_menu_labels(&items, Some(id));
            self.overlays.push(Overlay {
                root: dropdown,
                anchor,
                owner: Some(id),
                dismiss_outside: true,
                window_margin: flexui_geometry::Insets::default(),
                entries,
                style,
                parent_item: None,
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
                if w.base().role == WidgetRole::Radio
                    && w.selection_group() == Some(g)
                    && w.base().id != id
                {
                    w.base_mut().selected = false;
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
                    visit_mut(root, tb_id, &mut |w| { w.set_selected_index(ti); });
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
    if node.children_viewport().contains(p) {
        for child in b.children.iter().rev() {
            if let Some(id) = hit_test(child.as_ref(), p) {
                return Some(id);
            }
        }
    }
    match b.hit {
        HitPolicy::Solid => Some(b.id),
        HitPolicy::Transparent => None,
    }
}

/// 读取控件的可动画属性值。
/// 写入控件的可动画属性值（带各自的取值约束）。
/// 计算浮层摆放矩形：优先锚点下方、放不下则上翻；X 夹到窗内；至少 min_width 宽。
fn place_overlay(
    anchor: Rect,
    desired: Size,
    window: Size,
    min_width: f32,
    margin: flexui_geometry::Insets,
    alignment: crate::widgets::MenuAlignment,
) -> Rect {
    let available_w = (window.width - margin.horizontal()).max(0.0);
    let available_h = (window.height - margin.vertical()).max(0.0);
    let w = desired.width.max(min_width).min(available_w);
    let h = desired.height.min(available_h);
    // X：锚点左对齐，超出右边则左移，再夹到 0。
    let mut x = match alignment {
        crate::widgets::MenuAlignment::Start => anchor.left(),
        crate::widgets::MenuAlignment::End => anchor.right() - w,
    };
    if x + w > window.width - margin.right {
        x = window.width - margin.right - w;
    }
    if x < margin.left {
        x = margin.left;
    }
    // Y：优先锚点下方；放不下则上翻到上方；再放不下贴底。
    let below = anchor.bottom();
    let y = if below + h <= window.height - margin.bottom {
        below
    } else {
        let above = anchor.top() - h;
        if above >= margin.top {
            above
        } else {
            (window.height - margin.bottom - h).max(margin.top)
        }
    };
    Rect::new(x, y, w, h)
}

/// 子菜单优先贴父项右侧展开，空间不足时翻到左侧，Y 与父项顶边对齐。
fn place_submenu(
    anchor: Rect,
    desired: Size,
    window: Size,
    margin: flexui_geometry::Insets,
) -> Rect {
    let available_w = (window.width - margin.horizontal()).max(0.0);
    let available_h = (window.height - margin.vertical()).max(0.0);
    let w = desired.width.min(available_w);
    let h = desired.height.min(available_h);
    let x = if anchor.right() + w <= window.width - margin.right {
        anchor.right()
    } else {
        (anchor.left() - w).max(margin.left)
    };
    let y = anchor.top().min(window.height - margin.bottom - h).max(margin.top);
    Rect::new(x, y, w, h)
}

/// 滚动光标下最深的可滚动控件，返回实际发生变化的视口矩形。
fn scroll_tree_at(root: &mut dyn Widget, pos: Point, dy: f32) -> Option<Rect> {
    let mut target = None;
    for_each_visible_mut(root, true, &mut |widget| {
        if widget.is_scrollable() && widget.children_viewport().contains(pos) {
            target = Some(widget.base().id);
        }
    });
    let id = target?;
    let mut changed_rect = None;
    visit_mut(root, id, &mut |widget| {
        let viewport = widget.children_viewport();
        if widget.scroll_by(dy) {
            changed_rect = Some(viewport);
        }
    });
    changed_rect
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

/// 该角色是否接收指针（按下/拖动）事件转发：文本框、滑块、列表。
fn is_pointer_target(role: Option<WidgetRole>) -> bool {
    matches!(
        role,
        Some(WidgetRole::Edit) | Some(WidgetRole::Slider) | Some(WidgetRole::ListView)
    )
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
        let base = node.base();
        return Some(base.style.shadows().fold(base.rect, |visual, shadow| {
            union_rect(
                visual,
                Rect::new(
                    base.rect.left() + shadow.dx,
                    base.rect.top() + shadow.dy,
                    base.rect.size.width,
                    base.rect.size.height,
                ),
            )
        }));
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

/// 前序遍历有效可见子树；隐藏父节点的后代不会被访问。
fn for_each_visible_mut(
    node: &mut dyn Widget,
    ancestors_visible: bool,
    f: &mut dyn FnMut(&mut dyn Widget),
) {
    let visible = ancestors_visible && node.base().visible;
    if !visible {
        return;
    }
    f(node);
    let n = node.base().children.len();
    for i in 0..n {
        for_each_visible_mut(node.base_mut().children[i].as_mut(), visible, f);
    }
}

/// id 对应控件是否位于有效可见子树，且自身可聚焦、可用。
fn is_focus_candidate(node: &dyn Widget, id: WidgetId, ancestors_visible: bool) -> bool {
    let b = node.base();
    let visible = ancestors_visible && b.visible;
    if !visible {
        return false;
    }
    if b.id == id {
        return b.focusable && b.enabled;
    }
    b.children
        .iter()
        .any(|child| is_focus_candidate(child.as_ref(), id, visible))
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
        Button, ComboBox, Edit, Label, ListView, Panel, Progress, Radio, ScrollView, Slider,
        TabBox, VBox,
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
        assert_eq!(root.base().children[0].base().rect.top(), 0.0);

        let mut disp = Dispatcher::new();
        disp.handle(
            &mut root,
            &Event::MouseWheel { pos: Point::new(50.0, 50.0), dx: 0.0, dy: -60.0 },
        );
        assert_eq!(root.scroll_position(), Some(60.0)); // 视口100 内容200 → 可滚到 100，60 有效
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
        assert_eq!(root.cursor(), 3);
        assert_eq!(root.selection(), None);
        // 拖到 x≈8 → 边界1；选区 (1,3) = "el"。
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(8.0, 20.0) });
        assert_eq!(root.selection(), Some((1, 3)));
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
        assert!((root.current() - 0.5).abs() < 1e-3, "got {}", root.current());
        // 拖到 x=80 → 0.8
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(80.0, 10.0) });
        assert!((root.current() - 0.8).abs() < 1e-3);
        // 越界夹取
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(200.0, 10.0) });
        assert_eq!(root.current(), 1.0);
    }

    #[test]
    fn place_overlay_下方上翻夹取() {
        let win = Size::new(200.0, 100.0);
        // 下方够放：y=锚点底部；宽取 max(desired,min)。
        let r = place_overlay(Rect::new(10.0, 10.0, 50.0, 20.0), Size::new(40.0, 30.0), win, 50.0, flexui_geometry::Insets::default(), crate::widgets::MenuAlignment::Start);
        assert_eq!(r.top(), 30.0);
        assert_eq!(r.size.width, 50.0);
        // 下方放不下 → 上翻到锚点上方。
        let r2 = place_overlay(Rect::new(10.0, 80.0, 50.0, 15.0), Size::new(40.0, 30.0), win, 0.0, flexui_geometry::Insets::default(), crate::widgets::MenuAlignment::Start);
        assert_eq!(r2.top(), 50.0);
        // 锚点靠右 → X 夹到窗内。
        let r3 = place_overlay(Rect::new(180.0, 10.0, 10.0, 10.0), Size::new(40.0, 10.0), win, 0.0, flexui_geometry::Insets::default(), crate::widgets::MenuAlignment::Start);
        assert_eq!(r3.left(), 160.0);
        // 原版登录菜单在 580 宽窗口内保留 14px 右边距。
        let r4 = place_overlay(
            Rect::new(326.0, 116.0, 68.0, 44.0),
            Size::new(294.0, 228.0),
            Size::new(580.0, 416.0),
            68.0,
            flexui_geometry::Insets::new(0.0, 0.0, 14.0, 28.0),
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
            panel_padding: flexui_geometry::Insets::all(4.0),
            ..Default::default()
        };
        let items = (0..8)
            .map(|i| (format!("item {i}"), format!("item_{i}")))
            .collect();
        let mut disp = Dispatcher::new();
        disp.open_styled_menu(
            Rect::new(20.0, 20.0, 80.0, 20.0),
            items,
            Some(style),
            None,
        );
        disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
        let menu_viewport = disp.overlays[0].root.children_viewport();
        let main_before = root.scroll_position();
        disp.handle(
            &mut root,
            &Event::MouseWheel {
                pos: Point::new(menu_viewport.left() + 20.0, menu_viewport.top() + 20.0),
                dx: 0.0,
                dy: -32.0,
            },
        );
        assert!(disp.has_overlays());
        assert_eq!(disp.overlays[0].root.scroll_position(), Some(32.0));
        assert_eq!(root.scroll_position(), main_before);
    }

    #[test]
    fn 滚出子视口的菜单项不能命中() {
        let style = crate::widgets::MenuStyle {
            width: Some(160.0),
            height: Some(100.0),
            row_height: 32.0,
            panel_padding: flexui_geometry::Insets::new(10.0, 16.0, 10.0, 16.0),
            ..Default::default()
        };
        let items = (0..5)
            .map(|i| (format!("item {i}"), format!("item_{i}")))
            .collect::<Vec<_>>();
        let mut menu = crate::widgets::build_menu_styled(&items, None, &style, None);
        layout_node(menu.as_mut(), Rect::new(0.0, 0.0, 160.0, 100.0), &FakeCanvas);
        assert!(menu.scroll_by(-20.0));
        let padding_point = Point::new(20.0, 10.0);
        assert!(menu.base().children[0].base().rect.contains(padding_point));
        assert_ne!(hit_test(menu.as_ref(), padding_point), Some(menu.base().children[0].base().id));
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
    fn 右键菜单_选项按项名上报() {
        let mut root = VBox::new().push(Panel::new().size(100.0, 100.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
        let mut disp = Dispatcher::new();
        disp.open_menu(
            Rect::new(10.0, 10.0, 0.0, 0.0),
            vec![("复制".to_string(), "copy".to_string()), ("粘贴".to_string(), "paste".to_string())],
        );
        assert!(disp.has_overlays());
        disp.paint_overlays(&mut FakeCanvas, Size::new(300.0, 300.0));
        // 点第 2 项 "粘贴"。
        let c = disp.top_overlay_item_center(1).unwrap();
        disp.handle(&mut root, &Event::MouseDown { pos: c, button: MouseButton::Left });
        disp.handle(&mut root, &Event::MouseUp { pos: c, button: MouseButton::Left });
        assert!(!disp.has_overlays());
        assert_eq!(disp.take_activations(), vec!["paste".to_string()]);
    }

    #[test]
    fn listview_点击选中并上报激活() {
        let mut root = VBox::new().push(ListView::new().name("lv").items(["a", "b", "c"]).row_height(20.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 200.0, 200.0), &cv);
        let mut disp = Dispatcher::new();
        // 点击第 2 行（y≈50 → row 2，列表在 (0,0,200,200)）。
        disp.handle(&mut root, &Event::MouseDown { pos: Point::new(20.0, 50.0), button: MouseButton::Left });
        assert_eq!(disp.take_activations(), vec!["lv".to_string()]);
        assert_eq!(root.base().children[0].selected_index(), Some(2));
    }

    #[test]
    fn 主线程邮箱_跨线程投递与取走() {
        let disp = Dispatcher::new();
        let p = disp.proxy();
        p.send("a");
        let p2 = p.clone();
        std::thread::spawn(move || p2.send("b")).join().unwrap();
        let mut msgs = disp.drain_messages();
        msgs.sort();
        assert_eq!(msgs, vec!["a".to_string(), "b".to_string()]);
        assert!(disp.drain_messages().is_empty(), "取走后清空");
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
        let v = root.base().children[0].animation_value(AnimProp::Value).unwrap();
        assert!((v - 0.5).abs() < 1e-3, "got {v}");
        // 再 0.5s → 1.0 且结束
        disp.tick_anims(&mut root, 0.5);
        assert!((root.base().children[0].animation_value(AnimProp::Value).unwrap() - 1.0).abs() < 1e-3);
        assert!(!disp.has_anims());
        // 结束后无变化
        assert!(!disp.tick_anims(&mut root, 0.5));
    }

    #[test]
    fn tooltip_延时显示与移开清除() {
        let mut root = VBox::new().push(Button::new("b").name("bt").tooltip("提示").size(100.0, 30.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
        let mut disp = Dispatcher::new();
        // 无 hover → 不显示。
        assert!(disp.tooltip_tick(&mut root).is_none());
        // hover 到按钮。
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(50.0, 15.0) });
        // 一个 tick 后显示。
        assert!(disp.tooltip_tick(&mut root).is_some());
        assert!(disp.has_tooltip());
        // 已显示：再 tick 不重复。
        assert!(disp.tooltip_tick(&mut root).is_none());
        // hover 移开 → 立即清除。
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(250.0, 250.0) });
        assert!(!disp.has_tooltip());
    }

    #[test]
    fn tooltip_无提示文本不显示() {
        let mut root = VBox::new().push(Button::new("b").size(100.0, 30.0));
        let cv = FakeCanvas;
        layout_node(&mut root, Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
        let mut disp = Dispatcher::new();
        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(50.0, 15.0) });
        assert!(disp.tooltip_tick(&mut root).is_none());
        assert!(!disp.has_tooltip());
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
        assert_eq!(disp.overlays[1].root.base().rect.top(), disp.overlays[0].root.base().rect.top());
        assert!(disp.take_activations().is_empty());

        let child = disp.top_overlay_item_center(1).unwrap();
        disp.handle(&mut root, &Event::MouseDown { pos: child, button: MouseButton::Left });
        disp.handle(&mut root, &Event::MouseUp { pos: child, button: MouseButton::Left });
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
        disp.handle(&mut root, &Event::MouseDown {
            pos: Point::new(2.0, 10.0), button: MouseButton::Left,
        });
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
        disp.handle(&mut root, &Event::DoubleClick { pos: Point::new(40.0, 20.0) });
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
    fn hover_脏区包含所有状态阴影() {
        use crate::style::{BaseState, Shadow, StyleSet, StyleSpec};
        use flexui_geometry::Color;

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

        disp.handle(&mut root, &Event::MouseMove { pos: Point::new(50.0, 20.0) });

        assert_eq!(disp.take_dirty(), Some(Rect::new(0.0, 0.0, 208.0, 46.0)));
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
        assert_eq!(root.base().children[2].selected_index(), Some(1));
    }
}
