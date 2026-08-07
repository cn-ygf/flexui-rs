//! 控件库（L4）。所有控件内嵌 `Base`，共享统一的绘制/布局/分发管线。

mod containers;
mod controls;

pub use containers::{HBox, Panel, TabBox, VBox};
pub use controls::{Button, CheckBox, Edit, Image, Label, Radio};

/// 为控件生成一组通用 Builder 方法，减少样板。要求结构体有名为 `base` 的字段。
#[macro_export]
macro_rules! common_builders {
    ($t:ty) => {
        impl $t {
            /// 设置 name（供 XML/查找用）。
            pub fn name(mut self, n: impl Into<String>) -> Self {
                self.base.name = Some(n.into());
                self
            }
            /// 设置样式集。
            pub fn style(mut self, s: $crate::style::StyleSet) -> Self {
                self.base.style = s;
                self
            }
            /// 固定宽高。
            pub fn size(mut self, w: f32, h: f32) -> Self {
                self.base.width = Some(w);
                self.base.height = Some(h);
                self
            }
            /// 固定宽。
            pub fn width(mut self, w: f32) -> Self {
                self.base.width = Some(w);
                self
            }
            /// 固定高。
            pub fn height(mut self, h: f32) -> Self {
                self.base.height = Some(h);
                self
            }
            /// 四边内边距。
            pub fn padding(mut self, p: f32) -> Self {
                self.base.padding = flexui_geometry::Insets::all(p);
                self
            }
            /// flex 伸缩系数。
            pub fn flex(mut self, g: f32) -> Self {
                self.base.flex_grow = g;
                self
            }
            /// 是否可用（disabled 状态）。
            pub fn enabled(mut self, e: bool) -> Self {
                self.base.enabled = e;
                self
            }
            /// 命中策略（穿透/不穿透）。
            pub fn hit(mut self, h: $crate::widget::HitPolicy) -> Self {
                self.base.hit = h;
                self
            }
            /// 点击回调（参数为事件上下文，可按 name 访问/修改其它控件）。
            pub fn on_click(
                mut self,
                f: impl FnMut(&mut $crate::dispatch::EventCtx) + 'static,
            ) -> Self {
                self.base.on_click = Some(Box::new(f));
                self
            }
        }
    };
}
