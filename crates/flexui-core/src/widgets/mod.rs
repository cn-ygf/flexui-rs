//! 控件库（L4）：**按控件拆分文件**，每个控件一个模块。
//!
//! 所有控件内嵌 `Base` + 实现 `Widget`，共享统一的绘制/布局/分发管线；
//! 能力 trait（TextControl/Clickable/Container）表达类型层级（见 widget.rs）。
//!
//! - 基础控件：`label` `button` `image` `checkbox` `edit`
//! - 扩展控件：`radio`（+group / tabbar）
//! - 容器：`panel`(Box) `vbox` `hbox` `tabbox`

mod button;
mod checkbox;
mod combobox;
mod edit;
mod hbox;
mod image;
mod label;
mod listview;
mod menu;
mod panel;
mod progress;
mod radio;
mod scroll;
mod separator;
mod slider;
mod switch;
mod tabbox;
mod vbox;
mod virtual_list;

pub use button::Button;
pub use checkbox::CheckBox;
pub use combobox::ComboBox;
pub use edit::{Edit, EditConfig};
pub use hbox::HBox;
pub use image::Image;
pub use label::Label;
pub use listview::ListView;
pub use menu::{
    build_menu, build_menu_entries, build_menu_labels, build_menu_styled, build_tooltip,
    MenuAlignment, MenuEntry, MenuItem, MenuStyle,
};
pub use panel::Panel;
pub use progress::Progress;
pub use radio::Radio;
pub use scroll::{ScrollBarStyle, ScrollView};
pub use separator::Separator;
pub use slider::Slider;
pub use switch::Switch;
pub use tabbox::TabBox;
pub use vbox::VBox;
pub use virtual_list::{
    VirtualColumn, VirtualList, VirtualListRow, VirtualListRows, VirtualListSource,
    VirtualListSourceRef, VirtualSelectionMode, VirtualSort, VirtualSortDirection,
};

use flexui_gfx::{Color, Corners, Rect};
use flexui_gfx::{Canvas, TextAlign};

use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::widget::Base;

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
            /// 设置主题变体，例如 primary、danger、nav。
            pub fn variant(mut self, value: impl Into<String>) -> Self {
                self.base.variant = value.into();
                self
            }
            /// 追加一个主题类名。
            pub fn class(mut self, value: impl Into<String>) -> Self {
                self.base.classes.push(value.into());
                self
            }
            /// 替换全部主题类名。
            pub fn classes<I, S>(mut self, values: I) -> Self
            where
                I: IntoIterator<Item = S>,
                S: Into<String>,
            {
                self.base.classes = values.into_iter().map(Into::into).collect();
                self
            }
            /// 设置点击后播放的背景帧动画。
            pub fn click_bg_animation(mut self, animation: $crate::FrameAnimation) -> Self {
                self.base.click_bg_animation = Some(animation);
                self
            }
            /// 设置点击后播放的前景帧动画。
            pub fn click_fg_animation(mut self, animation: $crate::FrameAnimation) -> Self {
                self.base.click_fg_animation = Some(animation);
                self
            }
            /// 固定宽高。
            pub fn size(mut self, w: f32, h: f32) -> Self {
                self.base.width = $crate::sizing::Sizing::Fixed(w);
                self.base.height = $crate::sizing::Sizing::Fixed(h);
                self
            }
            /// 固定宽。
            pub fn width(mut self, w: f32) -> Self {
                self.base.width = $crate::sizing::Sizing::Fixed(w);
                self
            }
            /// 固定高。
            pub fn height(mut self, h: f32) -> Self {
                self.base.height = $crate::sizing::Sizing::Fixed(h);
                self
            }
            /// 设置宽度模式（Fixed/Content/Fill）。
            pub fn width_mode(mut self, s: $crate::sizing::Sizing) -> Self {
                self.base.width = s;
                self
            }
            /// 设置高度模式（Fixed/Content/Fill）。
            pub fn height_mode(mut self, s: $crate::sizing::Sizing) -> Self {
                self.base.height = s;
                self
            }
            /// 两轴都自动撑开（Fill）。
            pub fn fill(mut self) -> Self {
                self.base.width = $crate::sizing::Sizing::Fill;
                self.base.height = $crate::sizing::Sizing::Fill;
                self
            }
            /// 四边内边距（统一）。
            pub fn padding(mut self, p: f32) -> Self {
                self.base.padding = flexui_gfx::Insets::all(p);
                self
            }
            /// 内边距：水平(左右) + 垂直(上下)。
            pub fn padding_xy(mut self, horizontal: f32, vertical: f32) -> Self {
                self.base.padding =
                    flexui_gfx::Insets::new(horizontal, vertical, horizontal, vertical);
                self
            }
            /// 内边距：分别指定 左/上/右/下。
            pub fn padding_ltrb(mut self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
                self.base.padding = flexui_gfx::Insets::new(left, top, right, bottom);
                self
            }
            /// 四边外边距（统一）。
            pub fn margin(mut self, m: f32) -> Self {
                self.base.margin = flexui_gfx::Insets::all(m);
                self
            }
            /// 外边距：水平(左右) + 垂直(上下)。
            pub fn margin_xy(mut self, horizontal: f32, vertical: f32) -> Self {
                self.base.margin =
                    flexui_gfx::Insets::new(horizontal, vertical, horizontal, vertical);
                self
            }
            /// 外边距：分别指定 左/上/右/下。
            pub fn margin_ltrb(mut self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
                self.base.margin = flexui_gfx::Insets::new(left, top, right, bottom);
                self
            }
            /// 绝对定位（相对父内容区左上角；用于 Box 容器内）。
            pub fn pos(mut self, x: f32, y: f32) -> Self {
                self.base.pos = Some((x, y));
                self
            }
            /// 主轴对齐（VBox/HBox）。
            pub fn justify(mut self, j: $crate::sizing::Justify) -> Self {
                self.base.justify = j;
                self
            }
            /// 交叉轴对齐（VBox/HBox）。
            pub fn align(mut self, a: $crate::sizing::Align) -> Self {
                self.base.align = a;
                self
            }
            /// flex 伸缩系数（主轴 Fill 的分配权重）。
            pub fn flex(mut self, g: f32) -> Self {
                self.base.flex_grow = g;
                self
            }
            /// 字号。
            pub fn font_size(mut self, s: f32) -> Self {
                self.base.font.size = s;
                self
            }
            /// 字族名。
            pub fn font_family(mut self, name: impl Into<String>) -> Self {
                self.base.font.family = Some(name.into());
                self
            }
            /// 粗体。
            pub fn bold(mut self) -> Self {
                self.base.font.bold = true;
                self
            }
            /// 斜体。
            pub fn italic(mut self) -> Self {
                self.base.font.italic = true;
                self
            }
            /// 下划线。
            pub fn underline(mut self) -> Self {
                self.base.font.underline = true;
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
            /// 悬停提示文本（Tooltip）。
            pub fn tooltip(mut self, s: impl Into<String>) -> Self {
                self.base.tooltip = Some(s.into());
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
            /// 控件 hover、focus、文本、选择和值变化等语义事件。
            pub fn on_control_event(
                mut self,
                f: impl FnMut(&$crate::ControlEvent, &mut $crate::dispatch::EventCtx) + 'static,
            ) -> Self {
                self.base.on_control_event = Some(Box::new(f));
                self
            }
        }
    };
}

/// 共享：绘制左侧指示器（方框/圆点）+ 右侧文字。CheckBox/Radio 复用。
/// `circular=true` 画单选圆点，否则画勾选方框。
pub(crate) fn paint_indicator_and_text(
    base: &Base,
    cv: &mut dyn Canvas,
    style: &StyleSpec,
    circular: bool,
) {
    // 若该状态设了贴图（bg/fg image），则由统一管线绘制贴图，跳过默认指示器；
    // 只画右侧文字。→ 支持 duilib 式各状态贴图替换。
    if style.bg_image.is_some() || style.fg_image.is_some() {
        let content = crate::layout::content_rect(base);
        let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
        draw_aligned_text(
            cv,
            &base.text,
            content,
            &base.font,
            color,
            TextAlign::Left,
            true,
        );
        return;
    }

    let content = crate::layout::content_rect(base);
    let box_size = 16.0;
    let bx = content.left();
    let by = content.top() + (content.size.height - box_size) / 2.0;
    let ind = Rect::new(bx, by, box_size, box_size);
    let border = style
        .border_color
        .unwrap_or(Color::from_u8(140, 150, 170, 255));
    let radius = if circular {
        Corners::all(box_size / 2.0)
    } else {
        Corners::all(3.0)
    };
    // 指示器外框
    cv.stroke_round_rect(ind, radius, border, 1.5);
    // 选中：填充内部
    if base.selected {
        let fill = style
            .accent_color
            .or(style.fg_color)
            .unwrap_or(Color::from_u8(52, 120, 246, 255));
        let inner = ind.inset_all(4.0);
        let inner_radius = if circular {
            Corners::all((box_size - 8.0) / 2.0)
        } else {
            Corners::all(2.0)
        };
        cv.fill_round_rect(inner, inner_radius, fill);
    }
    // 文字
    let text_rect = Rect::new(
        content.left() + box_size + 8.0,
        content.top(),
        (content.size.width - box_size - 8.0).max(0.0),
        content.size.height,
    );
    let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
    draw_aligned_text(
        cv,
        &base.text,
        text_rect,
        &base.font,
        color,
        TextAlign::Left,
        true,
    );
}
