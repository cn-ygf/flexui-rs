//! 控件状态机与分状态样式（L3）。对应需求 C1/C2/C3。
//!
//! - 基础状态 4 种：Normal / Hot / Pushed / Disabled，优先级 Disabled>Pushed>Hot>Normal。
//! - focus 维度正交叠加：4×2 = 8 个样式槽。
//! - 每个样式槽 `StyleSpec` 所有字段可缺省，解析时按「状态回退 + 字段级回退到 Normal」补全。

use std::collections::HashMap;

use flexui_geometry::{Color, Corners};
use flexui_gfx::{ImageFit, ImageSource, TextAlign};

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

/// 单个状态槽的样式，所有字段可缺省（None 表示继承）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleSpec {
    pub bg_color: Option<Color>,
    pub fg_color: Option<Color>,
    pub bg_image: Option<ImageSource>,
    /// 背景图换色（None=原色）。
    pub bg_tint: Option<Color>,
    /// 背景图渲染方式。
    pub bg_fit: Option<ImageFit>,
    pub fg_image: Option<ImageSource>,
    pub fg_tint: Option<Color>,
    pub fg_fit: Option<ImageFit>,
    pub border_color: Option<Color>,
    pub border_width: Option<f32>,
    pub corner_radius: Option<Corners>,
    pub text_align: Option<TextAlign>,
}

impl StyleSpec {
    /// 用另一份样式做「兜底」：self 缺省的字段取 base 的值。
    fn fill_from(&self, base: &StyleSpec) -> StyleSpec {
        StyleSpec {
            bg_color: self.bg_color.or(base.bg_color),
            fg_color: self.fg_color.or(base.fg_color),
            bg_image: self.bg_image.clone().or_else(|| base.bg_image.clone()),
            bg_tint: self.bg_tint.or(base.bg_tint),
            bg_fit: self.bg_fit.clone().or_else(|| base.bg_fit.clone()),
            fg_image: self.fg_image.clone().or_else(|| base.fg_image.clone()),
            fg_tint: self.fg_tint.or(base.fg_tint),
            fg_fit: self.fg_fit.clone().or_else(|| base.fg_fit.clone()),
            border_color: self.border_color.or(base.border_color),
            border_width: self.border_width.or(base.border_width),
            corner_radius: self.corner_radius.or(base.corner_radius),
            text_align: self.text_align.or(base.text_align),
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

    /// 解析出生效样式：按「specific→general」回退链找生效槽，再字段级回退到 Normal。
    /// 回退链：全维度 → 去 focus → 去 selected → 仅 base。
    pub fn resolve(&self, state: VisualState) -> StyleSpec {
        let chain = [
            state,
            VisualState::with_selected(state.base, false, state.selected),
            VisualState::with_selected(state.base, state.focused, false),
            VisualState::new(state.base, false),
        ];
        let mut effective = StyleSpec::default();
        for cand in chain {
            if let Some(s) = self.slot(cand) {
                effective = s.clone();
                break;
            }
        }
        effective.fill_from(&self.normal)
    }
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
        set.set(VisualState::new(BaseState::Normal, false), spec_bg(Color::WHITE));
        // Hot 未设置 → 应回退到 Normal 的白色
        let r = set.resolve(VisualState::new(BaseState::Hot, false));
        assert_eq!(r.bg_color, Some(Color::WHITE));
    }

    #[test]
    fn focus_槽缺省回退到非_focus() {
        let mut set = StyleSet::new();
        set.set(VisualState::new(BaseState::Normal, false), spec_bg(Color::WHITE));
        set.set(VisualState::new(BaseState::Hot, false), spec_bg(Color::BLACK));
        // Hot+focus 未设置 → 回退到 Hot 非 focus 的黑色
        let r = set.resolve(VisualState::new(BaseState::Hot, true));
        assert_eq!(r.bg_color, Some(Color::BLACK));
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
        set.set(VisualState::new(BaseState::Hot, false), spec_bg(Color::BLACK));
        let r = set.resolve(VisualState::new(BaseState::Hot, false));
        assert_eq!(r.bg_color, Some(Color::BLACK)); // 用自己的
        assert_eq!(r.border_width, Some(1.0)); // 从 Normal 继承
    }
}
