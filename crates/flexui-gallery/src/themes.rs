//! Gallery 自定义主题示例：以品牌主色扩展语义色和控件配方。

use flexui::{Color, Corners, StyleSet, StyleSpec, Theme, WidgetKind};

pub(crate) const BILIBILI_PINK: Color =
    Color::rgba(251.0 / 255.0, 114.0 / 255.0, 153.0 / 255.0, 1.0);

/// 从默认亮色主题派生完整的哔哩哔哩粉主题。
///
/// `with_color` 覆盖语义色并自动重建全部内置控件配方；
/// `with_component_style` 则用于补充应用自己的控件外观。
pub(crate) fn bilibili_theme() -> Theme {
    let mut theme = Theme::light()
        .with_color("brand", BILIBILI_PINK)
        .with_color("brand-hover", rgb(252, 139, 171))
        .with_color("brand-pressed", rgb(226, 82, 123))
        .with_color("on-brand", rgb(255, 255, 255))
        .with_color("text-primary", rgb(24, 25, 28))
        .with_color("text-regular", rgb(44, 45, 48))
        .with_color("text-secondary", rgb(97, 102, 109))
        .with_color("text-placeholder", rgb(148, 153, 160))
        .with_color("text-disabled", rgb(194, 197, 202))
        .with_color("border", rgb(227, 229, 231))
        .with_color("border-hover", BILIBILI_PINK)
        .with_color("border-light", rgb(241, 242, 243))
        .with_color("fill", rgb(255, 255, 255))
        .with_color("fill-hover", rgb(255, 244, 247))
        .with_color("fill-pressed", rgb(255, 228, 236))
        .with_color("fill-disabled", rgb(246, 247, 248))
        .with_color("page", rgb(246, 247, 249))
        .with_color("surface", rgb(255, 255, 255))
        .with_color("overlay", rgba(255, 255, 255, 248))
        .with_color("success", rgb(42, 200, 100))
        .with_color("warning", rgb(255, 176, 39))
        .with_color("danger", rgb(248, 90, 84))
        .with_color("bilibili-pink-soft", rgb(255, 238, 243))
        .with_component_style(WidgetKind::Button, "default", rounded_control_style())
        .with_component_style(WidgetKind::Edit, "default", rounded_control_style())
        .with_component_style(WidgetKind::ComboBox, "default", rounded_control_style())
        .with_component_style(
            WidgetKind::Button,
            "variant:primary",
            rounded_control_style(),
        )
        .with_component_style(
            WidgetKind::Button,
            "class:theme-action",
            StyleSet::new().with_normal(StyleSpec {
                corner_radius: Some(Corners::all(6.0)),
                ..Default::default()
            }),
        );
    theme.name = "Bilibili Pink".to_owned();
    theme
        .with_component_style(
            WidgetKind::Radio,
            "variant:nav",
            StyleSet::new().with_normal(StyleSpec {
                corner_radius: Some(Corners::all(6.0)),
                ..Default::default()
            }),
        )
        .with_component_style(
            WidgetKind::ListView,
            "class:selectable",
            StyleSet::new().with_normal(StyleSpec {
                selection_color: Some(rgba(251, 114, 153, 54)),
                corner_radius: Some(Corners::all(6.0)),
                ..Default::default()
            }),
        )
}

fn rounded_control_style() -> StyleSet {
    StyleSet::new().with_normal(StyleSpec {
        corner_radius: Some(Corners::all(6.0)),
        ..Default::default()
    })
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_u8(r, g, b, 255)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_u8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui::{BaseState, VisualState, WidgetKind};

    #[test]
    fn bilibili_theme_覆盖语义色和控件配方() {
        let theme = bilibili_theme();
        assert_eq!(theme.name, "Bilibili Pink");
        assert_eq!(theme.color("brand"), Some(BILIBILI_PINK));
        assert_eq!(theme.color("bilibili-pink-soft"), Some(rgb(255, 238, 243)));

        let primary = theme
            .style_for(WidgetKind::Button, "primary", &[])
            .resolve(VisualState::new(BaseState::Normal, false));
        assert_eq!(primary.bg_color, Some(BILIBILI_PINK));
        assert_eq!(primary.corner_radius, Some(Corners::all(6.0)));

        let hot = theme
            .style_for(WidgetKind::Button, "primary", &[])
            .resolve(VisualState::new(BaseState::Hot, false));
        assert_eq!(hot.bg_color, Some(rgb(252, 139, 171)));
    }
}
