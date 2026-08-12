//! Menu：浮层菜单项 `MenuItem` 与构建器 `build_menu`（供下拉/右键菜单复用）。

use flexui_geometry::{Color, Corners, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::sizing::Sizing;
use crate::style::{StyleSet, StyleSpec};
use crate::widget::{Base, Node, Widget, WidgetRole};
use crate::widgets::VBox;

/// 菜单项高亮色（悬停）。
const HL_COLOR: Color = Color::rgba(0.28, 0.42, 0.70, 0.55);

/// 菜单项：浮层菜单中的一行。text=标签，selected_index=行号，hover 高亮。
pub struct MenuItem {
    base: Base,
    index: usize,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, index: usize) -> Self {
        let mut base = Base::new(WidgetRole::MenuItem);
        base.text = label.into();
        base.width = Sizing::Fill;
        base.height = Sizing::Fixed(28.0);
        base.padding = flexui_geometry::Insets::new(10.0, 0.0, 10.0, 0.0);
        Self { base, index }
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
        layout::size_from_content(&self.base, s.width, 28.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        // 悬停高亮铺满整行（含 padding 区）。
        if self.base.hover {
            cv.fill_rect(self.base.rect, HL_COLOR);
        }
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
        draw_aligned_text(cv, &self.base.text, content, &self.base.font, color, TextAlign::Left, true);
    }
    fn selected_index(&self) -> Option<usize> { Some(self.index) }
}

common_builders!(MenuItem);

/// 用 (标签, name) 列表构建一个浮层菜单（VBox + 若干 MenuItem）。
/// name 为空则该项不具名（下拉框场景由 owner 上报）。
pub fn build_menu(items: &[(String, String)], _owner: Option<crate::widget::WidgetId>) -> Node {
    // 菜单面板样式：深色背景 + 细边框 + 圆角。
    let panel = StyleSpec {
        bg_color: Some(Color::from_u8(38, 42, 52, 255)),
        border_color: Some(Color::from_u8(74, 80, 100, 255)),
        border_width: Some(1.0),
        corner_radius: Some(Corners::all(6.0)),
        ..Default::default()
    };
    let mut vbox = VBox::new().style(StyleSet::new().with_normal(panel));
    vbox = vbox.padding(4.0);
    for (i, (label, name)) in items.iter().enumerate() {
        let mut item = MenuItem::new(label.clone(), i);
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
}
