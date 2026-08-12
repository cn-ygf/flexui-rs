//! Menu：浮层菜单项 `MenuItem` 与构建器 `build_menu`（供下拉/右键菜单复用）。

use flexui_geometry::{Color, Corners, Insets, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::sizing::Sizing;
use crate::style::{StyleSet, StyleSpec};
use crate::widget::{Base, Node, Widget, WidgetRole};
use crate::widgets::VBox;

/// 浮层菜单的可配置外观。
#[derive(Debug, Clone)]
pub struct MenuStyle {
    pub background: Color,
    pub border: Color,
    pub text: Color,
    pub hot_text: Color,
    pub selected_text: Color,
    pub hot_background: Color,
    pub row_height: f32,
    pub width: Option<f32>,
    pub item_padding: Insets,
    pub panel_padding: f32,
    pub corner_radius: f32,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            background: Color::from_u8(38, 42, 52, 255),
            border: Color::from_u8(74, 80, 100, 255),
            text: Color::from_u8(230, 235, 245, 255),
            hot_text: Color::WHITE,
            selected_text: Color::WHITE,
            hot_background: Color::rgba(0.28, 0.42, 0.70, 0.55),
            row_height: 28.0,
            width: None,
            item_padding: Insets::new(10.0, 0.0, 10.0, 0.0),
            panel_padding: 4.0,
            corner_radius: 6.0,
        }
    }
}

/// 菜单项：浮层菜单中的一行。text=标签，selected_index=行号，hover 高亮。
pub struct MenuItem {
    base: Base,
    index: usize,
    selected_mark: bool,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, index: usize) -> Self {
        Self::styled(label, index, &MenuStyle::default(), false)
    }

    fn styled(label: impl Into<String>, index: usize, menu: &MenuStyle, selected: bool) -> Self {
        let mut base = Base::new(WidgetRole::MenuItem);
        base.text = label.into();
        base.width = Sizing::Fill;
        base.height = Sizing::Fixed(menu.row_height);
        base.padding = menu.item_padding;
        base.selected = selected;
        let mut styles = StyleSet::new().with_normal(StyleSpec {
            fg_color: Some(menu.text),
            ..Default::default()
        });
        styles.set(
            crate::style::VisualState::new(crate::style::BaseState::Hot, false),
            StyleSpec {
                bg_color: Some(menu.hot_background),
                fg_color: Some(menu.hot_text),
                ..Default::default()
            },
        );
        styles.set(
            crate::style::VisualState::with_selected(
                crate::style::BaseState::Normal,
                false,
                true,
            ),
            StyleSpec {
                fg_color: Some(menu.selected_text),
                ..Default::default()
            },
        );
        styles.set(
            crate::style::VisualState::with_selected(
                crate::style::BaseState::Hot,
                false,
                true,
            ),
            StyleSpec {
                bg_color: Some(menu.hot_background),
                fg_color: Some(menu.hot_text),
                ..Default::default()
            },
        );
        base.style = styles;
        Self { base, index, selected_mark: selected }
    }
}

impl Widget for MenuItem {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        layout::size_from_content(
            &self.base,
            s.width,
            self.base.height.fixed_value().unwrap_or(28.0),
        )
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
        if self.selected_mark {
            let mark_rect = flexui_geometry::Rect::new(
                self.base.rect.left() + 10.0,
                content.top(),
                14.0,
                content.size.height,
            );
            draw_aligned_text(cv, "✓", mark_rect, &self.base.font, color, TextAlign::Center, false);
        }
        draw_aligned_text(cv, &self.base.text, content, &self.base.font, color, TextAlign::Left, true);
    }
    fn selected_index(&self) -> Option<usize> { Some(self.index) }
}

common_builders!(MenuItem);

/// 用 (标签, name) 列表构建一个浮层菜单（VBox + 若干 MenuItem）。
/// name 为空则该项不具名（下拉框场景由 owner 上报）。
pub fn build_menu(items: &[(String, String)], _owner: Option<crate::widget::WidgetId>) -> Node {
    build_menu_styled(items, _owner, &MenuStyle::default(), None)
}

/// 用指定外观构建菜单，并按 name 标记当前选中项。
pub fn build_menu_styled(
    items: &[(String, String)],
    _owner: Option<crate::widget::WidgetId>,
    menu: &MenuStyle,
    selected_name: Option<&str>,
) -> Node {
    // 菜单面板样式：深色背景 + 细边框 + 圆角。
    let panel = StyleSpec {
        bg_color: Some(menu.background),
        border_color: Some(menu.border),
        border_width: Some(1.0),
        corner_radius: Some(Corners::all(menu.corner_radius)),
        ..Default::default()
    };
    let mut vbox = VBox::new().style(StyleSet::new().with_normal(panel));
    vbox = vbox.padding(menu.panel_padding);
    if let Some(width) = menu.width {
        vbox.base_mut().width = Sizing::Fixed(width);
    }
    for (i, (label, name)) in items.iter().enumerate() {
        let selected = selected_name.is_some_and(|selected| selected == name);
        let mut item = MenuItem::styled(label.clone(), i, menu, selected);
        if !name.is_empty() {
            item = item.name(name.clone());
        }
        vbox = vbox.push(item);
    }
    Box::new(vbox)
}

/// 便捷：由纯标签列表构建菜单（下拉框用；各项不具名）。
pub fn build_menu_labels(labels: &[String], owner: Option<crate::widget::WidgetId>) -> Node {
    let items: Vec<(String, String)> =
        labels.iter().map(|l| (l.clone(), String::new())).collect();
    build_menu(&items, owner)
}

/// 构建一个 Tooltip 提示气泡（深色底 + 边框 + 内边距的单行标签）。
pub fn build_tooltip(text: &str) -> Node {
    let style = StyleSpec {
        bg_color: Some(Color::from_u8(50, 54, 64, 245)),
        fg_color: Some(Color::from_u8(230, 235, 245, 255)),
        border_color: Some(Color::from_u8(90, 96, 116, 255)),
        border_width: Some(1.0),
        corner_radius: Some(Corners::all(4.0)),
        ..Default::default()
    };
    let lbl = crate::widgets::Label::new(text)
        .style(StyleSet::new().with_normal(style))
        .padding_xy(8.0, 4.0);
    Box::new(lbl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::WidgetRole;

    #[test]
    fn build_menu_行号与具名() {
        let items = vec![("A".to_string(), "a".to_string()), ("B".to_string(), String::new())];
        let node = build_menu(&items, None);
        let ch = &node.base().children;
        assert_eq!(ch.len(), 2);
        assert_eq!(ch[0].base().role, WidgetRole::MenuItem);
        assert_eq!(ch[0].selected_index(), Some(0));
        assert_eq!(ch[1].selected_index(), Some(1));
        assert_eq!(ch[0].base().name.as_deref(), Some("a"));
        assert_eq!(ch[1].base().name, None, "空 name 不具名");
    }

    #[test]
    fn styled_menu_标记当前项并应用状态颜色() {
        let style = MenuStyle {
            background: Color::from_u8(48, 54, 110, 255),
            text: Color::from_u8(188, 192, 212, 255),
            hot_background: Color::from_u8(71, 75, 133, 255),
            row_height: 32.0,
            width: Some(294.0),
            ..Default::default()
        };
        let items = vec![
            ("中国大陆  +86".to_string(), "nation_86".to_string()),
            ("中国香港  +852".to_string(), "nation_852".to_string()),
        ];
        let node = build_menu_styled(&items, None, &style, Some("nation_852"));

        assert!(!node.base().children[0].base().selected);
        assert!(node.base().children[1].base().selected);
        assert_eq!(node.base().children[1].base().height, Sizing::Fixed(32.0));
        assert_eq!(
            node.base().children[0].base().resolved_style().fg_color,
            Some(style.text)
        );
    }
}
