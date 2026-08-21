//! 无贴图主题：语义色板、控件配方及控件树主题应用。

use std::collections::HashMap;

use flexui_gfx::{Color, Corners};
use flexui_gfx::TextAlign;

use crate::style::{BaseState, StyleSet, StyleSpec, VisualState};
use crate::widget::Widget;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

/// 控件外观类型，与事件分发使用的 WidgetRole 相互独立。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    Panel,
    VBox,
    HBox,
    Label,
    Button,
    CheckBox,
    Switch,
    Radio,
    TabBox,
    Edit,
    Image,
    Progress,
    Slider,
    ComboBox,
    ListView,
    VirtualList,
    ScrollView,
    Separator,
    MenuItem,
}

/// XML 主题令牌可绑定的颜色属性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeColorProperty {
    Background,
    Foreground,
    Border,
    BackgroundTint,
    ForegroundTint,
    Accent,
    Thumb,
    Track,
    Selection,
    Scrollbar,
    Placeholder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeColorBinding {
    pub state: VisualState,
    pub property: ThemeColorProperty,
    pub token: String,
}

/// 常用颜色使用明确字段，业务自定义令牌存入 custom。
#[derive(Debug, Clone)]
pub struct ThemePalette {
    pub brand: Color,
    pub brand_hover: Color,
    pub brand_pressed: Color,
    pub on_brand: Color,
    pub text_primary: Color,
    pub text_regular: Color,
    pub text_secondary: Color,
    pub text_placeholder: Color,
    pub text_disabled: Color,
    pub border: Color,
    pub border_hover: Color,
    pub border_light: Color,
    pub fill: Color,
    pub fill_hover: Color,
    pub fill_pressed: Color,
    pub fill_disabled: Color,
    pub page: Color,
    pub surface: Color,
    pub overlay: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub custom: HashMap<String, Color>,
}

impl ThemePalette {
    pub fn color(&self, token: &str) -> Option<Color> {
        let token = normalize_token(token);
        Some(match token.as_str() {
            "brand" => self.brand,
            "brand_hover" => self.brand_hover,
            "brand_pressed" => self.brand_pressed,
            "on_brand" => self.on_brand,
            "text_primary" => self.text_primary,
            "text_regular" => self.text_regular,
            "text_secondary" => self.text_secondary,
            "text_placeholder" => self.text_placeholder,
            "text_disabled" => self.text_disabled,
            "border" => self.border,
            "border_hover" => self.border_hover,
            "border_light" => self.border_light,
            "fill" => self.fill,
            "fill_hover" => self.fill_hover,
            "fill_pressed" => self.fill_pressed,
            "fill_disabled" => self.fill_disabled,
            "page" => self.page,
            "surface" => self.surface,
            "overlay" => self.overlay,
            "success" => self.success,
            "warning" => self.warning,
            "danger" => self.danger,
            _ => *self.custom.get(&token)?,
        })
    }

    pub fn set_color(&mut self, token: impl AsRef<str>, color: Color) {
        let token = normalize_token(token.as_ref());
        match token.as_str() {
            "brand" => self.brand = color,
            "brand_hover" => self.brand_hover = color,
            "brand_pressed" => self.brand_pressed = color,
            "on_brand" => self.on_brand = color,
            "text_primary" => self.text_primary = color,
            "text_regular" => self.text_regular = color,
            "text_secondary" => self.text_secondary = color,
            "text_placeholder" => self.text_placeholder = color,
            "text_disabled" => self.text_disabled = color,
            "border" => self.border = color,
            "border_hover" => self.border_hover = color,
            "border_light" => self.border_light = color,
            "fill" => self.fill = color,
            "fill_hover" => self.fill_hover = color,
            "fill_pressed" => self.fill_pressed = color,
            "fill_disabled" => self.fill_disabled = color,
            "page" => self.page = color,
            "surface" => self.surface = color,
            "overlay" => self.overlay = color,
            "success" => self.success = color,
            "warning" => self.warning = color,
            "danger" => self.danger = color,
            _ => {
                self.custom.insert(token, color);
            }
        }
    }
}

/// 一套完整主题。selector 支持 default、variant:name 和 class:name。
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub mode: ThemeMode,
    pub palette: ThemePalette,
    builtin_component_styles: HashMap<(WidgetKind, String), StyleSet>,
    component_styles: HashMap<(WidgetKind, String), StyleSet>,
}

impl Theme {
    pub fn light() -> Self {
        Self::builtin(
            "FlexUI Light",
            ThemeMode::Light,
            ThemePalette {
                brand: rgb(64, 119, 255),
                brand_hover: rgb(84, 134, 255),
                brand_pressed: rgb(45, 96, 224),
                on_brand: rgb(255, 255, 255),
                text_primary: rgb(31, 34, 40),
                text_regular: rgb(69, 74, 84),
                text_secondary: rgb(123, 129, 141),
                text_placeholder: rgb(155, 161, 173),
                text_disabled: rgb(174, 179, 188),
                border: rgb(205, 210, 220),
                border_hover: rgb(116, 145, 216),
                border_light: rgb(230, 233, 239),
                fill: rgb(255, 255, 255),
                fill_hover: rgb(246, 248, 252),
                fill_pressed: rgb(235, 239, 247),
                fill_disabled: rgb(242, 244, 247),
                page: rgb(247, 248, 250),
                surface: rgb(255, 255, 255),
                overlay: rgba(255, 255, 255, 248),
                success: rgb(35, 158, 96),
                warning: rgb(218, 145, 25),
                danger: rgb(216, 69, 79),
                custom: HashMap::new(),
            },
        )
    }

    pub fn dark() -> Self {
        Self::builtin(
            "FlexUI Dark",
            ThemeMode::Dark,
            ThemePalette {
                brand: rgb(92, 142, 255),
                brand_hover: rgb(116, 159, 255),
                brand_pressed: rgb(67, 117, 230),
                on_brand: rgb(255, 255, 255),
                text_primary: rgb(241, 243, 247),
                text_regular: rgb(214, 218, 226),
                text_secondary: rgb(157, 164, 178),
                text_placeholder: rgb(126, 133, 147),
                text_disabled: rgb(100, 106, 118),
                border: rgb(76, 82, 95),
                border_hover: rgb(111, 139, 204),
                border_light: rgb(56, 61, 72),
                fill: rgb(42, 46, 55),
                fill_hover: rgb(52, 57, 68),
                fill_pressed: rgb(61, 67, 80),
                fill_disabled: rgb(45, 49, 58),
                page: rgb(28, 31, 37),
                surface: rgb(35, 39, 47),
                overlay: rgba(42, 46, 55, 248),
                success: rgb(71, 190, 126),
                warning: rgb(235, 174, 69),
                danger: rgb(238, 103, 111),
                custom: HashMap::new(),
            },
        )
    }

    fn builtin(name: &str, mode: ThemeMode, palette: ThemePalette) -> Self {
        let mut theme = Self {
            name: name.to_owned(),
            mode,
            palette,
            builtin_component_styles: HashMap::new(),
            component_styles: HashMap::new(),
        };
        theme.rebuild_builtin_styles();
        theme
    }

    /// 修改语义色后同步重建内置控件配方。
    pub fn with_color(mut self, token: impl AsRef<str>, color: Color) -> Self {
        self.palette.set_color(token, color);
        self.rebuild_builtin_styles();
        self
    }

    pub fn with_component_style(
        mut self,
        kind: WidgetKind,
        selector: impl Into<String>,
        style: StyleSet,
    ) -> Self {
        self.set_component_style(kind, selector, style);
        self
    }

    pub fn set_component_style(
        &mut self,
        kind: WidgetKind,
        selector: impl Into<String>,
        style: StyleSet,
    ) {
        let selector = normalize_selector(selector.into());
        self.component_styles.insert((kind, selector), style);
    }

    pub fn color(&self, token: &str) -> Option<Color> {
        self.palette.color(token)
    }

    pub fn style_for(&self, kind: WidgetKind, variant: &str, classes: &[String]) -> StyleSet {
        let mut result = self.component_style(kind, "default");
        if !variant.is_empty() {
            result = self
                .component_style(kind, &format!("variant:{variant}"))
                .merged_over(&result);
        }
        for class in classes {
            result = self
                .component_style(kind, &format!("class:{class}"))
                .merged_over(&result);
        }
        result
    }

    fn component_style(&self, kind: WidgetKind, selector: &str) -> StyleSet {
        let builtin = self
            .builtin_component_styles
            .get(&(kind, selector.to_owned()))
            .cloned()
            .unwrap_or_default();
        self.component_styles
            .get(&(kind, selector.to_owned()))
            .map(|style| style.merged_over(&builtin))
            .unwrap_or(builtin)
    }

    fn set_builtin_component_style(
        &mut self,
        kind: WidgetKind,
        selector: impl Into<String>,
        style: StyleSet,
    ) {
        self.builtin_component_styles
            .insert((kind, normalize_selector(selector.into())), style);
    }

    fn rebuild_builtin_styles(&mut self) {
        self.builtin_component_styles.clear();
        let p = self.palette.clone();
        let text = StyleSet::new()
            .with_normal(spec_fg(p.text_regular))
            .with_state(BaseState::Disabled, spec_fg(p.text_disabled));
        self.set_builtin_component_style(WidgetKind::Label, "default", text.clone());
        let selection = StyleSet::new()
            .with_normal(StyleSpec {
                fg_color: Some(p.text_regular),
                border_color: Some(p.border),
                accent_color: Some(p.brand),
                thumb_color: Some(p.on_brand),
                ..Default::default()
            })
            .with_state(
                BaseState::Hot,
                StyleSpec {
                    border_color: Some(p.border_hover),
                    ..Default::default()
                },
            )
            .with_state(
                BaseState::Disabled,
                StyleSpec {
                    fg_color: Some(p.text_disabled),
                    border_color: Some(p.border_light),
                    accent_color: Some(p.text_disabled),
                    ..Default::default()
                },
            );
        self.set_builtin_component_style(WidgetKind::CheckBox, "default", selection.clone());
        self.set_builtin_component_style(WidgetKind::Radio, "default", selection);
        let mut switch = StyleSet::new().with_normal(StyleSpec {
            track_color: Some(p.border),
            thumb_color: Some(p.on_brand),
            ..Default::default()
        });
        switch.set(
            VisualState::new(BaseState::Hot, false),
            StyleSpec {
                track_color: Some(p.border_hover),
                ..Default::default()
            },
        );
        switch.set(
            VisualState::new(BaseState::Pushed, false),
            StyleSpec {
                track_color: Some(p.text_secondary),
                ..Default::default()
            },
        );
        switch.set(
            VisualState::new(BaseState::Disabled, false),
            StyleSpec {
                track_color: Some(p.border_light),
                ..Default::default()
            },
        );
        switch.set(
            VisualState::with_selected(BaseState::Normal, false, true),
            StyleSpec {
                track_color: Some(p.brand),
                ..Default::default()
            },
        );
        switch.set(
            VisualState::with_selected(BaseState::Hot, false, true),
            StyleSpec {
                track_color: Some(p.brand_hover),
                ..Default::default()
            },
        );
        switch.set(
            VisualState::with_selected(BaseState::Pushed, false, true),
            StyleSpec {
                track_color: Some(p.brand_pressed),
                ..Default::default()
            },
        );
        switch.set(
            VisualState::with_selected(BaseState::Disabled, false, true),
            StyleSpec {
                track_color: Some(p.text_disabled),
                ..Default::default()
            },
        );
        self.set_builtin_component_style(WidgetKind::Switch, "default", switch);

        let bordered = StyleSet::new()
            .with_normal(StyleSpec {
                bg_color: Some(p.fill),
                fg_color: Some(p.text_regular),
                border_color: Some(p.border),
                border_width: Some(1.0),
                corner_radius: Some(Corners::all(4.0)),
                accent_color: Some(p.brand),
                placeholder_color: Some(p.text_placeholder),
                ..Default::default()
            })
            .with_state(
                BaseState::Hot,
                StyleSpec {
                    bg_color: Some(p.fill_hover),
                    border_color: Some(p.border_hover),
                    ..Default::default()
                },
            )
            .with_state(
                BaseState::Pushed,
                StyleSpec {
                    bg_color: Some(p.fill_pressed),
                    ..Default::default()
                },
            )
            .with_state(
                BaseState::Disabled,
                StyleSpec {
                    bg_color: Some(p.fill_disabled),
                    fg_color: Some(p.text_disabled),
                    border_color: Some(p.border_light),
                    ..Default::default()
                },
            );
        for kind in [WidgetKind::Button, WidgetKind::Edit, WidgetKind::ComboBox] {
            self.set_builtin_component_style(kind, "default", bordered.clone());
        }

        self.set_builtin_component_style(
            WidgetKind::Button,
            "variant:primary",
            solid_button(p.brand, p.brand_hover, p.brand_pressed, p.on_brand),
        );
        self.set_builtin_component_style(
            WidgetKind::Button,
            "variant:danger",
            solid_button(
                p.danger,
                tint(p.danger, 0.10),
                tint(p.danger, -0.10),
                rgb(255, 255, 255),
            ),
        );
        let mut nav = StyleSet::new()
            .with_normal(StyleSpec {
                fg_color: Some(p.text_secondary),
                text_align: Some(TextAlign::Center),
                corner_radius: Some(Corners::all(4.0)),
                ..Default::default()
            })
            .with_state(
                BaseState::Hot,
                StyleSpec {
                    bg_color: Some(p.fill_hover),
                    fg_color: Some(p.text_primary),
                    ..Default::default()
                },
            );
        nav.set(
            VisualState::with_selected(BaseState::Normal, false, true),
            StyleSpec {
                bg_color: Some(p.fill_pressed),
                fg_color: Some(p.brand),
                ..Default::default()
            },
        );
        self.set_builtin_component_style(WidgetKind::Radio, "variant:nav", nav);

        let progress = StyleSet::new().with_normal(StyleSpec {
            bg_color: Some(p.border_light),
            accent_color: Some(p.brand),
            ..Default::default()
        });
        self.set_builtin_component_style(WidgetKind::Progress, "default", progress);
        self.set_builtin_component_style(
            WidgetKind::Slider,
            "default",
            StyleSet::new().with_normal(StyleSpec {
                track_color: Some(p.border_light),
                accent_color: Some(p.brand),
                thumb_color: Some(p.on_brand),
                border_color: Some(p.border),
                ..Default::default()
            }),
        );
        self.set_builtin_component_style(
            WidgetKind::ListView,
            "default",
            StyleSet::new()
                .with_normal(StyleSpec {
                    bg_color: Some(p.fill),
                    fg_color: Some(p.text_regular),
                    border_color: Some(p.border),
                    border_width: Some(1.0),
                    corner_radius: Some(Corners::all(4.0)),
                    scrollbar_color: Some(p.text_secondary),
                    ..Default::default()
                })
                .with_state(
                    BaseState::Disabled,
                    StyleSpec {
                        bg_color: Some(p.fill_disabled),
                        fg_color: Some(p.text_disabled),
                        border_color: Some(p.border_light),
                        ..Default::default()
                    },
                ),
        );
        self.set_builtin_component_style(
            WidgetKind::VirtualList,
            "default",
            StyleSet::new()
                .with_normal(StyleSpec {
                    bg_color: Some(p.fill),
                    fg_color: Some(p.text_regular),
                    border_color: Some(p.border),
                    border_width: Some(1.0),
                    corner_radius: Some(Corners::all(4.0)),
                    accent_color: Some(p.brand),
                    selection_color: Some(with_alpha(p.brand, 48)),
                    scrollbar_color: Some(p.text_secondary),
                    ..Default::default()
                })
                .with_state(
                    BaseState::Disabled,
                    StyleSpec {
                        bg_color: Some(p.fill_disabled),
                        fg_color: Some(p.text_disabled),
                        border_color: Some(p.border_light),
                        ..Default::default()
                    },
                ),
        );
        self.set_builtin_component_style(
            WidgetKind::Separator,
            "default",
            StyleSet::new().with_normal(StyleSpec {
                bg_color: Some(p.border_light),
                ..Default::default()
            }),
        );
        self.set_builtin_component_style(
            WidgetKind::ScrollView,
            "default",
            StyleSet::new().with_normal(StyleSpec {
                scrollbar_color: Some(p.text_secondary),
                ..Default::default()
            }),
        );
        self.set_builtin_component_style(
            WidgetKind::ListView,
            "class:selectable",
            StyleSet::new().with_normal(StyleSpec {
                selection_color: Some(with_alpha(p.brand, 70)),
                ..Default::default()
            }),
        );
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

/// 重算整棵控件树主题层；根节点额外获得页面底色。
pub fn apply_theme(root: &mut dyn Widget, theme: &Theme) {
    apply_theme_shared(root, Arc::new(theme.clone()), true);
}

/// 为运行时插入的子树应用主题，不附加窗口根页面底色。
pub(crate) fn apply_theme_subtree(root: &mut dyn Widget, theme: &Theme) {
    apply_theme_shared(root, Arc::new(theme.clone()), false);
}

/// 使用控件已保存的主题重新计算当前节点及其子树。
pub fn refresh_theme(node: &mut dyn Widget) {
    let Some(theme) = node.base().applied_theme.clone() else {
        return;
    };
    apply_theme_shared(node, theme, false);
}

/// 仅重算一个 Base，供属性 API 修改 variant/class 后即时刷新。
pub(crate) fn refresh_theme_base(base: &mut crate::widget::Base) {
    let Some(theme) = base.applied_theme.clone() else {
        return;
    };
    apply_theme_to_base(base, &theme, base.theme_root);
}

fn apply_theme_shared(node: &mut dyn Widget, theme: Arc<Theme>, is_root: bool) {
    let base = node.base_mut();
    apply_theme_to_base(base, &theme, is_root);
    for child in &mut base.children {
        apply_theme_shared(child.as_mut(), theme.clone(), false);
    }
}

fn apply_theme_to_base(base: &mut crate::widget::Base, theme: &Arc<Theme>, is_root: bool) {
    let mut style = theme.style_for(base.kind, &base.variant, &base.classes);
    if is_root {
        let root_style = StyleSet::new().with_normal(StyleSpec {
            bg_color: Some(theme.palette.page),
            fg_color: Some(theme.palette.text_regular),
            ..Default::default()
        });
        style = style.merged_over(&root_style);
    }
    if !base.theme_colors.is_empty() {
        let mut tokens = StyleSet::new();
        for binding in &base.theme_colors {
            let Some(color) = theme.color(&binding.token) else {
                continue;
            };
            let mut spec = tokens.resolve(binding.state);
            set_theme_color(&mut spec, binding.property, color);
            tokens.set(binding.state, spec);
        }
        style = tokens.merged_over(&style);
    }
    base.theme_style = style;
    base.applied_theme = Some(theme.clone());
    base.theme_root = is_root;
}

fn set_theme_color(spec: &mut StyleSpec, property: ThemeColorProperty, color: Color) {
    match property {
        ThemeColorProperty::Background => spec.bg_color = Some(color),
        ThemeColorProperty::Foreground => spec.fg_color = Some(color),
        ThemeColorProperty::Border => spec.border_color = Some(color),
        ThemeColorProperty::BackgroundTint => spec.bg_tint = Some(color),
        ThemeColorProperty::ForegroundTint => spec.fg_tint = Some(color),
        ThemeColorProperty::Accent => spec.accent_color = Some(color),
        ThemeColorProperty::Thumb => spec.thumb_color = Some(color),
        ThemeColorProperty::Track => spec.track_color = Some(color),
        ThemeColorProperty::Selection => spec.selection_color = Some(color),
        ThemeColorProperty::Scrollbar => spec.scrollbar_color = Some(color),
        ThemeColorProperty::Placeholder => spec.placeholder_color = Some(color),
    }
}

fn normalize_token(token: &str) -> String {
    token.trim().trim_start_matches('@').replace('-', "_")
}

fn normalize_selector(selector: String) -> String {
    let value = selector.trim();
    if value.is_empty() {
        "default".to_owned()
    } else {
        value.to_owned()
    }
}

fn spec_fg(color: Color) -> StyleSpec {
    StyleSpec {
        fg_color: Some(color),
        ..Default::default()
    }
}

fn solid_button(normal: Color, hot: Color, pushed: Color, fg: Color) -> StyleSet {
    StyleSet::new()
        .with_normal(StyleSpec {
            bg_color: Some(normal),
            fg_color: Some(fg),
            border_color: Some(normal),
            border_width: Some(1.0),
            corner_radius: Some(Corners::all(4.0)),
            ..Default::default()
        })
        .with_state(
            BaseState::Hot,
            StyleSpec {
                bg_color: Some(hot),
                border_color: Some(hot),
                ..Default::default()
            },
        )
        .with_state(
            BaseState::Pushed,
            StyleSpec {
                bg_color: Some(pushed),
                border_color: Some(pushed),
                ..Default::default()
            },
        )
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_u8(r, g, b, 255)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_u8(r, g, b, a)
}

fn with_alpha(color: Color, alpha: u8) -> Color {
    Color::rgba(color.r, color.g, color.b, alpha as f32 / 255.0)
}

fn tint(color: Color, amount: f32) -> Color {
    let mix = |value: f32| {
        if amount >= 0.0 {
            value + (1.0 - value) * amount
        } else {
            value * (1.0 + amount)
        }
    };
    Color::rgba(mix(color.r), mix(color.g), mix(color.b), color.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Invalidation, Panel, WindowCtx, WindowHandle};

    #[test]
    fn variant_overrides_default_component_style() {
        let theme = Theme::light();
        let style = theme.style_for(WidgetKind::Button, "primary", &[]);
        let resolved = style.resolve(VisualState::default());
        assert_eq!(resolved.bg_color, Some(theme.palette.brand));
        assert_eq!(resolved.fg_color, Some(theme.palette.on_brand));
    }

    #[test]
    fn custom_token_can_be_read_back() {
        let color = rgb(1, 2, 3);
        let theme = Theme::light().with_color("business-accent", color);
        assert_eq!(theme.color("@business_accent"), Some(color));
    }

    #[test]
    fn palette_update_keeps_application_component_styles() {
        let custom = StyleSet::new().with_normal(StyleSpec {
            border_width: Some(3.0),
            ..Default::default()
        });
        let theme = Theme::light()
            .with_component_style(WidgetKind::Button, "class:custom", custom)
            .with_color("brand", rgb(10, 20, 30));
        let resolved = theme
            .style_for(WidgetKind::Button, "", &["custom".to_owned()])
            .resolve(VisualState::default());
        assert_eq!(resolved.border_width, Some(3.0));
        assert_eq!(theme.palette.brand, rgb(10, 20, 30));
    }

    #[test]
    fn checkbox_slider_不绘制控件整体背景() {
        let theme = Theme::light();
        for kind in [WidgetKind::CheckBox, WidgetKind::Slider] {
            let resolved = theme
                .style_for(kind, "", &[])
                .resolve(VisualState::default());
            assert_eq!(resolved.bg_color, None, "{kind:?}");
        }
        let slider = theme
            .style_for(WidgetKind::Slider, "", &[])
            .resolve(VisualState::default());
        assert!(slider.track_color.is_some());
    }

    #[test]
    fn switch_选中状态只改变内部轨道() {
        let theme = Theme::light();
        let style = theme.style_for(WidgetKind::Switch, "", &[]);
        let normal = style.resolve(VisualState::default());
        let selected = style.resolve(VisualState::with_selected(BaseState::Normal, false, true));
        assert_eq!(normal.bg_color, None);
        assert_eq!(selected.bg_color, None);
        assert_eq!(normal.track_color, Some(theme.palette.border));
        assert_eq!(selected.track_color, Some(theme.palette.brand));
    }

    #[test]
    fn listview_按下不改变整体背景() {
        let theme = Theme::light();
        let style = theme.style_for(WidgetKind::ListView, "", &[]);
        let normal = style.resolve(VisualState::default());
        let pushed = style.resolve(VisualState::new(BaseState::Pushed, false));
        assert_eq!(pushed.bg_color, normal.bg_color);
        assert_eq!(pushed.border_color, normal.border_color);
    }

    struct TestWindow;

    impl WindowHandle for TestWindow {
        fn set_title(&mut self, _title: &str) {}
        fn close(&mut self) {}
        fn minimize(&mut self) {}
        fn maximize(&mut self) {}
        fn restore(&mut self) {}
    }

    #[test]
    fn window_context_switches_theme_and_requests_layout() {
        let mut root = Panel::new();
        let mut window = TestWindow;
        let mut ctx = WindowCtx::new(&mut root, &mut window);
        ctx.set_theme(Theme::dark());
        assert_eq!(ctx.theme().unwrap().mode, ThemeMode::Dark);
        assert_eq!(ctx.take_invalidation(), Invalidation::Layout);
    }
}
