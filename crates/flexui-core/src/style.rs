//! 控件状态机与分状态样式（L3）。对应需求 C1/C2/C3。
//!
//! - 基础状态 4 种：Normal / Hot / Pushed / Disabled，优先级 Disabled>Pushed>Hot>Normal。
//! - focus 维度正交叠加：4×2 = 8 个样式槽。
//! - 每个样式槽 `StyleSpec` 所有字段可缺省，解析时按状态层叠并逐字段继承。

use std::collections::HashMap;

use flexui_geometry::{Color, Corners};
use flexui_gfx::{Font, ImageFit, ImageSource, TextAlign};

use crate::frame_animation::FrameAnimation;

/// 基础状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BaseState {
    #[default]
    Normal,
    Hot,
    Pushed,
    Disabled,
}

/// 完整视觉状态：基础状态 + 是否 focus + 是否 selected（勾选/单选选中）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VisualState {
    pub base: BaseState,
    pub focused: bool,
    pub selected: bool,
}

impl VisualState {
    /// 构造（selected 默认 false）。
    pub fn new(base: BaseState, focused: bool) -> Self {
        Self {
            base,
            focused,
            selected: false,
        }
    }
    /// 带 selected 维度构造（用于 CheckBox/Radio 贴图）。
    pub fn with_selected(base: BaseState, focused: bool, selected: bool) -> Self {
        Self {
            base,
            focused,
            selected,
        }
    }
}

/// 线性渐变（两色）：竖直（上→下）或水平（左→右）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    pub from: Color,
    pub to: Color,
    pub vertical: bool,
}

/// 投影（硬阴影：按 dx/dy 偏移的同形填充，无模糊）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub color: Color,
}

/// 单个状态槽的样式，所有字段可缺省（None 表示继承）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleSpec {
    pub bg_color: Option<Color>,
    pub fg_color: Option<Color>,
    pub bg_image: Option<ImageSource>,
    /// 背景帧动画；活动时替换 `bg_image`。
    pub bg_animation: Option<FrameAnimation>,
    /// 背景图换色（None=原色）。
    pub bg_tint: Option<Color>,
    /// 背景图渲染方式。
    pub bg_fit: Option<ImageFit>,
    pub fg_image: Option<ImageSource>,
    /// 前景帧动画；活动时替换 `fg_image`。
    pub fg_animation: Option<FrameAnimation>,
    pub fg_tint: Option<Color>,
    pub fg_fit: Option<ImageFit>,
    pub border_color: Option<Color>,
    pub border_width: Option<f32>,
    /// 进度、已填充轨道及选择指示器等使用的强调色。
    pub accent_color: Option<Color>,
    /// 滑块拖柄等可移动指示器的填充色。
    pub thumb_color: Option<Color>,
    /// 列表选中行等局部选择区域的背景色。
    pub selection_color: Option<Color>,
    /// 控件内部滚动条颜色。
    pub scrollbar_color: Option<Color>,
    /// Edit 未设置专属占位样式时使用的主题占位文字色。
    pub placeholder_color: Option<Color>,
    pub corner_radius: Option<Corners>,
    pub text_align: Option<TextAlign>,
    /// 控件自身透明度 0~1（作用于本控件的背景/内容/边框，不含子控件）。
    pub opacity: Option<f32>,
    /// 背景线性渐变（存在时优先于 bg_color）。
    pub gradient: Option<Gradient>,
    /// 投影。
    pub shadow: Option<Shadow>,
}

/// Edit 占位文本的单个状态样式。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaceholderStyleSpec {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub fg_color: Option<Color>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
}

impl PlaceholderStyleSpec {
    fn fill_from(&self, base: &PlaceholderStyleSpec) -> Self {
        Self {
            font_family: self
                .font_family
                .clone()
                .or_else(|| base.font_family.clone()),
            font_size: self.font_size.or(base.font_size),
            fg_color: self.fg_color.or(base.fg_color),
            bold: self.bold.or(base.bold),
            italic: self.italic.or(base.italic),
            underline: self.underline.or(base.underline),
        }
    }

    /// 把稀疏字体样式覆盖到控件字体上。
    pub fn resolve_font(&self, base: &Font) -> Font {
        Font {
            family: self.font_family.clone().or_else(|| base.family.clone()),
            size: self.font_size.unwrap_or(base.size),
            bold: self.bold.unwrap_or(base.bold),
            italic: self.italic.unwrap_or(base.italic),
            underline: self.underline.unwrap_or(base.underline),
        }
    }
}

/// Edit 占位文本的分状态样式集。
#[derive(Debug, Clone, Default)]
pub struct PlaceholderStyleSet {
    normal: PlaceholderStyleSpec,
    slots: HashMap<VisualState, PlaceholderStyleSpec>,
}

impl PlaceholderStyleSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, state: VisualState, spec: PlaceholderStyleSpec) {
        if state == VisualState::default() {
            self.normal = spec;
        } else {
            self.slots.insert(state, spec);
        }
    }

    pub fn with_normal(mut self, spec: PlaceholderStyleSpec) -> Self {
        self.normal = spec;
        self
    }

    fn slot(&self, state: VisualState) -> Option<&PlaceholderStyleSpec> {
        if state == VisualState::default() {
            Some(&self.normal)
        } else {
            self.slots.get(&state)
        }
    }

    /// 按视觉状态逐层叠加：normal → base → focus → base+focus → selected 组合。
    pub fn resolve(&self, state: VisualState) -> PlaceholderStyleSpec {
        let mut effective = self.normal.clone();
        for candidate in style_layers(state) {
            if let Some(spec) = self.slot(candidate) {
                effective = spec.fill_from(&effective);
            }
        }
        effective
    }
}

impl StyleSpec {
    /// 用另一份样式做「兜底」：self 缺省的字段取 base 的值。
    pub(crate) fn fill_from(&self, base: &StyleSpec) -> StyleSpec {
        StyleSpec {
            bg_color: self.bg_color.or(base.bg_color),
            fg_color: self.fg_color.or(base.fg_color),
            bg_image: self.bg_image.clone().or_else(|| base.bg_image.clone()),
            bg_animation: self
                .bg_animation
                .clone()
                .or_else(|| base.bg_animation.clone()),
            bg_tint: self.bg_tint.or(base.bg_tint),
            bg_fit: self.bg_fit.clone().or_else(|| base.bg_fit.clone()),
            fg_image: self.fg_image.clone().or_else(|| base.fg_image.clone()),
            fg_animation: self
                .fg_animation
                .clone()
                .or_else(|| base.fg_animation.clone()),
            fg_tint: self.fg_tint.or(base.fg_tint),
            fg_fit: self.fg_fit.clone().or_else(|| base.fg_fit.clone()),
            border_color: self.border_color.or(base.border_color),
            border_width: self.border_width.or(base.border_width),
            accent_color: self.accent_color.or(base.accent_color),
            thumb_color: self.thumb_color.or(base.thumb_color),
            selection_color: self.selection_color.or(base.selection_color),
            scrollbar_color: self.scrollbar_color.or(base.scrollbar_color),
            placeholder_color: self.placeholder_color.or(base.placeholder_color),
            corner_radius: self.corner_radius.or(base.corner_radius),
            text_align: self.text_align.or(base.text_align),
            opacity: self.opacity.or(base.opacity),
            gradient: self.gradient.or(base.gradient),
            shadow: self.shadow.or(base.shadow),
        }
    }
}

/// 分状态样式集合：稀疏 map，键为 (base, focus, selected)；未命中按回退链取，
/// 最终字段级回退到 Normal 槽。
#[derive(Debug, Clone, Default)]
pub struct StyleSet {
    normal: StyleSpec,
    slots: HashMap<VisualState, StyleSpec>,
}

impl StyleSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置某状态槽。
    pub fn set(&mut self, state: VisualState, spec: StyleSpec) {
        if state == VisualState::default() {
            self.normal = spec;
        } else {
            self.slots.insert(state, spec);
        }
    }

    /// 便捷：设置 normal 槽。
    pub fn with_normal(mut self, spec: StyleSpec) -> Self {
        self.normal = spec;
        self
    }

    /// 便捷：设置某基础状态（无 focus/selected）槽。
    pub fn with_state(mut self, base: BaseState, spec: StyleSpec) -> Self {
        self.set(VisualState::new(base, false), spec);
        self
    }

    /// 取某状态槽（不做回退）。
    fn slot(&self, state: VisualState) -> Option<&StyleSpec> {
        if state == VisualState::default() {
            Some(&self.normal)
        } else {
            self.slots.get(&state)
        }
    }

    /// 解析出生效样式：从 Normal 开始逐层叠加基础、focus、selected 及组合状态。
    /// 后叠加的状态只覆盖自身声明的字段，因此 focus 可覆盖 hot，同时继承 hot 的其它字段。
    pub fn resolve(&self, state: VisualState) -> StyleSpec {
        let mut effective = self.normal.clone();
        for candidate in style_layers(state) {
            if let Some(spec) = self.slot(candidate) {
                effective = spec.fill_from(&effective);
            }
        }
        effective
    }

    /// 先解析 base，再让当前样式集逐字段覆盖。
    pub fn resolve_over(&self, base: &StyleSet, state: VisualState) -> StyleSpec {
        self.resolve(state).fill_from(&base.resolve(state))
    }

    /// 把当前样式集叠加在 base 之上，并保留全部视觉状态。
    pub fn merged_over(&self, base: &StyleSet) -> StyleSet {
        let mut result = StyleSet::new();
        for state in all_visual_states() {
            result.set(state, self.resolve_over(base, state));
        }
        result
    }

    /// 所有状态槽可能产生的阴影，用于计算状态切换时的完整视觉脏区。
    pub(crate) fn shadows(&self) -> impl Iterator<Item = Shadow> + '_ {
        std::iter::once(self.normal.shadow)
            .chain(self.slots.values().map(|slot| slot.shadow))
            .flatten()
    }
}

fn all_visual_states() -> impl Iterator<Item = VisualState> {
    [
        BaseState::Normal,
        BaseState::Hot,
        BaseState::Pushed,
        BaseState::Disabled,
    ]
    .into_iter()
    .flat_map(|base| {
        [false, true].into_iter().flat_map(move |focused| {
            [false, true]
                .into_iter()
                .map(move |selected| VisualState::with_selected(base, focused, selected))
        })
    })
}

fn style_layers(state: VisualState) -> impl Iterator<Item = VisualState> {
    let has_base = state.base != BaseState::Normal;
    [
        has_base.then(|| VisualState::new(state.base, false)),
        state
            .focused
            .then(|| VisualState::new(BaseState::Normal, true)),
        (has_base && state.focused).then(|| VisualState::new(state.base, true)),
        state
            .selected
            .then(|| VisualState::with_selected(BaseState::Normal, false, true)),
        (has_base && state.selected).then(|| VisualState::with_selected(state.base, false, true)),
        (state.focused && state.selected)
            .then(|| VisualState::with_selected(BaseState::Normal, true, true)),
        (has_base && state.focused && state.selected)
            .then(|| VisualState::with_selected(state.base, true, true)),
    ]
    .into_iter()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui_geometry::Color;

    fn spec_bg(c: Color) -> StyleSpec {
        StyleSpec {
            bg_color: Some(c),
            ..Default::default()
        }
    }

    #[test]
    fn 未设置状态回退到_normal() {
        let mut set = StyleSet::new();
        set.set(
            VisualState::new(BaseState::Normal, false),
            spec_bg(Color::WHITE),
        );
        // Hot 未设置 → 应回退到 Normal 的白色
        let r = set.resolve(VisualState::new(BaseState::Hot, false));
        assert_eq!(r.bg_color, Some(Color::WHITE));
    }

    #[test]
    fn focus_槽缺省回退到非_focus() {
        let mut set = StyleSet::new();
        set.set(
            VisualState::new(BaseState::Normal, false),
            spec_bg(Color::WHITE),
        );
        set.set(
            VisualState::new(BaseState::Hot, false),
            spec_bg(Color::BLACK),
        );
        // Hot+focus 未设置 → 回退到 Hot 非 focus 的黑色
        let r = set.resolve(VisualState::new(BaseState::Hot, true));
        assert_eq!(r.bg_color, Some(Color::BLACK));
    }

    #[test]
    fn focus_样式覆盖hot且继承其余字段() {
        let mut set = StyleSet::new();
        set.set(
            VisualState::new(BaseState::Hot, false),
            StyleSpec {
                bg_color: Some(Color::BLACK),
                border_width: Some(2.0),
                ..Default::default()
            },
        );
        set.set(
            VisualState::new(BaseState::Normal, true),
            spec_bg(Color::WHITE),
        );

        let resolved = set.resolve(VisualState::new(BaseState::Hot, true));
        assert_eq!(resolved.bg_color, Some(Color::WHITE));
        assert_eq!(resolved.border_width, Some(2.0));
    }

    #[test]
    fn hot_focus_组合样式优先于单独状态() {
        let mut set = StyleSet::new();
        let combined = Color::from_u8(255, 0, 0, 255);
        set.set(
            VisualState::new(BaseState::Hot, false),
            spec_bg(Color::BLACK),
        );
        set.set(
            VisualState::new(BaseState::Normal, true),
            spec_bg(Color::WHITE),
        );
        set.set(VisualState::new(BaseState::Hot, true), spec_bg(combined));

        let resolved = set.resolve(VisualState::new(BaseState::Hot, true));
        assert_eq!(resolved.bg_color, Some(combined));
    }

    #[test]
    fn 字段级回退_只缺的字段取_normal() {
        let mut set = StyleSet::new();
        set.set(
            VisualState::new(BaseState::Normal, false),
            StyleSpec {
                bg_color: Some(Color::WHITE),
                border_width: Some(1.0),
                ..Default::default()
            },
        );
        // Hot 只设置了 bg，其它字段（border_width）应从 Normal 继承
        set.set(
            VisualState::new(BaseState::Hot, false),
            spec_bg(Color::BLACK),
        );
        let r = set.resolve(VisualState::new(BaseState::Hot, false));
        assert_eq!(r.bg_color, Some(Color::BLACK)); // 用自己的
        assert_eq!(r.border_width, Some(1.0)); // 从 Normal 继承
    }

    #[test]
    fn placeholder_state_fields_fall_back_to_normal() {
        let mut set = PlaceholderStyleSet::new().with_normal(PlaceholderStyleSpec {
            font_family: Some("Microsoft YaHei".to_string()),
            font_size: Some(14.0),
            fg_color: Some(Color::WHITE),
            bold: Some(true),
            italic: Some(false),
            underline: Some(true),
        });
        set.set(
            VisualState::new(BaseState::Hot, false),
            PlaceholderStyleSpec {
                font_size: Some(16.0),
                fg_color: Some(Color::BLACK),
                italic: Some(true),
                ..Default::default()
            },
        );
        let resolved = set.resolve(VisualState::new(BaseState::Hot, true));
        assert_eq!(resolved.font_family.as_deref(), Some("Microsoft YaHei"));
        assert_eq!(resolved.font_size, Some(16.0));
        assert_eq!(resolved.fg_color, Some(Color::BLACK));
        assert_eq!(resolved.bold, Some(true));
        assert_eq!(resolved.italic, Some(true));
        assert_eq!(resolved.underline, Some(true));
    }

    #[test]
    fn placeholder_focus_overrides_hot_fields() {
        let mut set = PlaceholderStyleSet::new();
        set.set(
            VisualState::new(BaseState::Hot, false),
            PlaceholderStyleSpec {
                font_size: Some(16.0),
                fg_color: Some(Color::BLACK),
                ..Default::default()
            },
        );
        set.set(
            VisualState::new(BaseState::Normal, true),
            PlaceholderStyleSpec {
                fg_color: Some(Color::WHITE),
                ..Default::default()
            },
        );

        let resolved = set.resolve(VisualState::new(BaseState::Hot, true));
        assert_eq!(resolved.font_size, Some(16.0));
        assert_eq!(resolved.fg_color, Some(Color::WHITE));
    }
}
