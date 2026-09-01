use super::*;

/// 事件上下文：传给控件回调（on_click），可按 name 访问/修改整棵控件树。
///
/// 因此按钮点击能改别的控件（如更新状态标签），实现「可见的事件响应」。
pub struct EventCtx<'a> {
    root: &'a mut dyn Widget,
    invalidation: Invalidation,
}

impl<'a> EventCtx<'a> {
    pub(crate) fn new(root: &'a mut dyn Widget) -> Self {
        Self {
            root,
            invalidation: Invalidation::None,
        }
    }

    pub(crate) fn take_invalidation(&mut self) -> Invalidation {
        std::mem::take(&mut self.invalidation)
    }

    fn invalidate(&mut self, invalidation: Invalidation) {
        self.invalidation = self.invalidation.merge(invalidation);
    }

    fn rect(&self, name: &str) -> Option<Rect> {
        let id = find_by_name(self.root, name)?;
        rect_of(self.root, id)
    }

    fn mutate<R>(&mut self, name: &str, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
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

    /// 只读访问具名控件，不产生刷新请求。
    pub fn get<R>(&self, name: &str, f: impl FnOnce(&dyn Widget) -> R) -> Option<R> {
        let id = find_by_name(self.root, name)?;
        find_by_id(self.root, id).map(f)
    }

    /// 对名为 name 的控件执行 f；找到则返回 Some(f 的返回值)。
    pub fn with<R>(&mut self, name: &str, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        let out = self.mutate(name, f);
        if out.is_some() {
            self.invalidate(Invalidation::Layout);
        }
        out
    }

    /// 修改只影响绘制的属性。框架会自动重绘该控件区域。
    pub fn with_paint<R>(&mut self, name: &str, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        let rect = self.rect(name)?;
        let out = self.mutate(name, f);
        if out.is_some() {
            self.invalidate(Invalidation::Paint(rect));
        }
        out
    }

    /// 便捷：设置某控件的文本。
    pub fn set_text(&mut self, name: &str, text: impl Into<String>) {
        let text = text.into();
        let changed = self.get(name, |w| w.base().text != text).unwrap_or(false);
        if !changed {
            return;
        }
        self.with(name, move |w| {
            w.base_mut()
                .localizations
                .retain(|binding| !matches!(binding, crate::LocalizationBinding::Text(_)));
            w.set_text_value(text);
        });
    }

    /// 便捷：读取某控件的 selected（CheckBox/Radio）。
    pub fn is_selected(&mut self, name: &str) -> Option<bool> {
        self.get(name, |w| w.base().selected)
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.get(name, |w| w.base().text.clone())
    }

    pub fn is_enabled(&self, name: &str) -> Option<bool> {
        self.get(name, |w| w.base().enabled)
    }

    pub fn is_visible(&self, name: &str) -> Option<bool> {
        self.get(name, |w| w.base().visible)
    }

    /// 便捷：设置某控件是否可用。
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        if self.is_enabled(name) == Some(enabled) {
            return;
        }
        self.with_paint(name, move |w| w.base_mut().enabled = enabled);
    }

    /// 便捷：设置某控件及其子树是否参与布局、绘制与命中测试。
    pub fn set_visible(&mut self, name: &str, visible: bool) {
        if self.is_visible(name) == Some(visible) {
            return;
        }
        self.with(name, move |w| w.base_mut().visible = visible);
    }

    /// 设置 CheckBox/Radio 等控件的选中状态。
    pub fn set_selected(&mut self, name: &str, selected: bool) -> bool {
        if self.get(name, |w| w.base().selected) == Some(selected) {
            return false;
        }
        self.with_paint(name, move |w| w.base_mut().selected = selected)
            .is_some()
    }

    pub fn selected_index(&self, name: &str) -> Option<usize> {
        self.get(name, |w| w.selected_index()).flatten()
    }

    pub fn set_selected_index(&mut self, name: &str, index: usize) -> bool {
        let mut changed = false;
        let found = self
            .mutate(name, |w| changed = w.set_selected_index(index))
            .is_some();
        if found && changed {
            self.invalidate(Invalidation::Layout);
        }
        changed
    }

    pub fn value(&self, name: &str) -> Option<f32> {
        self.get(name, |w| w.animation_value(AnimProp::Value))
            .flatten()
    }

    pub fn set_value(&mut self, name: &str, value: f32) -> bool {
        let old = self.value(name);
        let mut supported = false;
        let rect = self.rect(name);
        self.mutate(name, |w| {
            supported = w.set_animation_value(AnimProp::Value, value)
        });
        let changed = supported && self.value(name) != old;
        if changed {
            self.invalidate(rect.map_or(Invalidation::Redraw, Invalidation::Paint));
        }
        changed
    }

    pub fn scroll_offset(&self, name: &str) -> Option<Point> {
        self.get(name, |w| w.scroll_offset()).flatten()
    }

    /// 让惰性数据控件重新读取数据源，并自动触发布局和重绘。
    pub fn refresh_data(&mut self, name: &str) -> bool {
        let mut refreshed = false;
        let found = self
            .mutate(name, |widget| refreshed = widget.refresh_data())
            .is_some();
        if found && refreshed {
            self.invalidate(Invalidation::Layout);
        }
        found && refreshed
    }

    /// 应用控件专属属性；保守地触发布局和整窗重绘。
    pub fn set_property(&mut self, name: &str, property: crate::widget::WidgetProperty) -> bool {
        use crate::widget::WidgetProperty as Property;
        match property {
            Property::Text(text) => {
                let found = self.get(name, |_| ()).is_some();
                self.set_text(name, text);
                found
            }
            Property::Tooltip(value) => {
                self.set_base_property(name, false, move |base| base.tooltip = value)
            }
            Property::Font(value) => {
                self.set_base_property(name, true, move |base| base.font = value)
            }
            Property::Style(value) => {
                self.set_base_property(name, false, move |base| base.style = value)
            }
            Property::Variant(value) => self.set_base_property(name, false, move |base| {
                base.variant = value;
                crate::theme::refresh_theme_base(base);
            }),
            Property::Classes(value) => self.set_base_property(name, false, move |base| {
                base.classes = value;
                crate::theme::refresh_theme_base(base);
            }),
            Property::Width(value) => {
                self.set_base_property(name, true, move |base| base.width = value)
            }
            Property::Height(value) => {
                self.set_base_property(name, true, move |base| base.height = value)
            }
            Property::Padding(value) => {
                self.set_base_property(name, true, move |base| base.padding = value)
            }
            Property::Margin(value) => {
                self.set_base_property(name, true, move |base| base.margin = value)
            }
            Property::Spacing(value) => {
                self.set_base_property(name, true, move |base| base.spacing = value)
            }
            Property::Flex(value) => {
                self.set_base_property(name, true, move |base| base.flex_grow = value)
            }
            Property::Position(value) => {
                self.set_base_property(name, true, move |base| base.pos = value)
            }
            Property::Justify(value) => {
                self.set_base_property(name, true, move |base| base.justify = value)
            }
            Property::Align(value) => {
                self.set_base_property(name, true, move |base| base.align = value)
            }
            Property::Enabled(value) => {
                let found = self.get(name, |_| ()).is_some();
                self.set_enabled(name, value);
                found
            }
            Property::Visible(value) => {
                let found = self.get(name, |_| ()).is_some();
                self.set_visible(name, value);
                found
            }
            Property::Focusable(value) => {
                self.set_base_property(name, false, move |base| base.focusable = value)
            }
            Property::FocusWithin(value) => {
                self.set_base_property(name, false, move |base| base.focus_within = value)
            }
            Property::HitPolicy(value) => {
                self.set_base_property(name, false, move |base| base.hit = value)
            }
            Property::Selected(selected) => self.set_selected(name, selected),
            property => {
                let mut applied = false;
                self.mutate(name, |w| applied = w.apply_property(property));
                if applied {
                    self.invalidate(Invalidation::Layout);
                }
                applied
            }
        }
    }

    fn set_base_property(
        &mut self,
        name: &str,
        layout: bool,
        update: impl FnOnce(&mut crate::widget::Base),
    ) -> bool {
        let rect = self.rect(name);
        let found = self
            .mutate(name, |widget| update(widget.base_mut()))
            .is_some();
        if found {
            self.invalidate(if layout {
                Invalidation::Layout
            } else {
                rect.map_or(Invalidation::Redraw, Invalidation::Paint)
            });
        }
        found
    }

    pub fn property(
        &self,
        name: &str,
        key: crate::widget::WidgetPropertyKey,
    ) -> Option<crate::widget::WidgetProperty> {
        use crate::widget::{WidgetProperty as Property, WidgetPropertyKey as Key};
        let common = self
            .get(name, |widget| {
                let base = widget.base();
                match key {
                    Key::Text => Some(Property::Text(base.text.clone())),
                    Key::Tooltip => Some(Property::Tooltip(base.tooltip.clone())),
                    Key::Font => Some(Property::Font(base.font.clone())),
                    Key::Style => Some(Property::Style(base.style.clone())),
                    Key::Variant => Some(Property::Variant(base.variant.clone())),
                    Key::Classes => Some(Property::Classes(base.classes.clone())),
                    Key::Width => Some(Property::Width(base.width)),
                    Key::Height => Some(Property::Height(base.height)),
                    Key::Padding => Some(Property::Padding(base.padding)),
                    Key::Margin => Some(Property::Margin(base.margin)),
                    Key::Spacing => Some(Property::Spacing(base.spacing)),
                    Key::Flex => Some(Property::Flex(base.flex_grow)),
                    Key::Position => Some(Property::Position(base.pos)),
                    Key::Justify => Some(Property::Justify(base.justify)),
                    Key::Align => Some(Property::Align(base.align)),
                    Key::Enabled => Some(Property::Enabled(base.enabled)),
                    Key::Visible => Some(Property::Visible(base.visible)),
                    Key::Focusable => Some(Property::Focusable(base.focusable)),
                    Key::FocusWithin => Some(Property::FocusWithin(base.focus_within)),
                    Key::HitPolicy => Some(Property::HitPolicy(base.hit)),
                    Key::Selected => Some(Property::Selected(base.selected)),
                    _ => None,
                }
            })
            .flatten();
        if common.is_some() {
            return common;
        }
        self.get(name, |w| w.property(key)).flatten()
    }

    /// 向容器末尾添加一个子控件。
    pub fn add_child(&mut self, name: &str, mut child: Node) -> bool {
        self.mutate(name, move |w| {
            if let Some(theme) = w.base().applied_theme.clone() {
                crate::theme::apply_theme_subtree(child.as_mut(), theme.as_ref());
            }
            w.base_mut().children.push(child);
        })
        .is_some_and(|_| {
            self.invalidate(Invalidation::Layout);
            true
        })
    }

    /// 删除并返回容器指定位置的子控件。
    pub fn remove_child(&mut self, name: &str, index: usize) -> Option<Node> {
        let child = self
            .mutate(name, |w| {
                (index < w.base().children.len()).then(|| w.base_mut().children.remove(index))
            })
            .flatten();
        if child.is_some() {
            self.invalidate(Invalidation::Layout);
        }
        child
    }

    /// 替换并返回容器指定位置的原子控件。
    pub fn replace_child(&mut self, name: &str, index: usize, mut child: Node) -> Option<Node> {
        let old = self
            .mutate(name, move |w| {
                if let Some(theme) = w.base().applied_theme.clone() {
                    crate::theme::apply_theme_subtree(child.as_mut(), theme.as_ref());
                }
                (index < w.base().children.len())
                    .then(|| std::mem::replace(&mut w.base_mut().children[index], child))
            })
            .flatten();
        if old.is_some() {
            self.invalidate(Invalidation::Layout);
        }
        old
    }

    /// 清空容器子控件并返回原节点。
    pub fn clear_children(&mut self, name: &str) -> Option<Vec<Node>> {
        let children = self.mutate(name, |w| std::mem::take(&mut w.base_mut().children))?;
        if !children.is_empty() {
            self.invalidate(Invalidation::Layout);
        }
        Some(children)
    }

    pub fn request_paint(&mut self, name: &str) -> bool {
        let Some(rect) = self.rect(name) else {
            return false;
        };
        self.invalidate(Invalidation::Paint(rect));
        true
    }

    pub fn request_redraw(&mut self) {
        self.invalidate(Invalidation::Redraw);
    }

    pub fn request_layout(&mut self) {
        self.invalidate(Invalidation::Layout);
    }

    /// 在指定控件上播放一段运行时帧动画。它覆盖状态图片，结束后按 `finish` 处理。
    pub fn play_frame_animation(
        &mut self,
        name: &str,
        layer: FrameLayer,
        animation: FrameAnimation,
    ) -> bool {
        self.with(name, move |w| match layer {
            FrameLayer::Background => w.base_mut().click_bg_frame_player.start(animation),
            FrameLayer::Foreground => w.base_mut().click_fg_frame_player.start(animation),
        })
        .is_some()
    }

    pub fn pause_frame_animation(&mut self, name: &str, layer: FrameLayer) -> bool {
        self.with(name, move |w| match layer {
            FrameLayer::Background => w.base_mut().click_bg_frame_player.pause(),
            FrameLayer::Foreground => w.base_mut().click_fg_frame_player.pause(),
        })
        .unwrap_or(false)
    }

    pub fn resume_frame_animation(&mut self, name: &str, layer: FrameLayer) -> bool {
        self.with(name, move |w| match layer {
            FrameLayer::Background => w.base_mut().click_bg_frame_player.resume(),
            FrameLayer::Foreground => w.base_mut().click_fg_frame_player.resume(),
        })
        .unwrap_or(false)
    }

    pub fn stop_frame_animation(&mut self, name: &str, layer: FrameLayer) -> bool {
        self.with(name, move |w| match layer {
            FrameLayer::Background => w.base_mut().click_bg_frame_player.stop(),
            FrameLayer::Foreground => w.base_mut().click_fg_frame_player.stop(),
        })
        .unwrap_or(false)
    }
}
