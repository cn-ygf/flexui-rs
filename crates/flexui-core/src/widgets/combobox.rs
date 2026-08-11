//! ComboBox：下拉选择框（点击弹出选项菜单，选中回填文本）。
//!
//! 选项存于本控件；点击时分发器读取 `menu_items()` 建浮层菜单，选中后回调
//! `set_selected_item()` 设 `selected_index` 并回填 `text`。

use flexui_geometry::{Color, Rect, Size};
use flexui_gfx::{Canvas, TextAlign};

use crate::common_builders;
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::widget::{Base, Widget, WidgetRole};

/// 下拉选择框：显示当前项 + 右侧 ▼，点击弹出选项菜单。
pub struct ComboBox {
    base: Base,
    options: Vec<String>,
}

impl ComboBox {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::ComboBox),
            options: Vec::new(),
        }
    }
    /// 设置选项列表；默认选中第 0 项并回填文本。
    pub fn options(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.options = items.into_iter().map(Into::into).collect();
        if self.base.text.is_empty() {
            if let Some(first) = self.options.first() {
                self.base.text = first.clone();
                self.base.selected_index = 0;
            }
        }
        self
    }
    /// 选中第 i 项（回填文本）。
    pub fn selected(mut self, i: usize) -> Self {
        self.apply_selection(i);
        self
    }

    /// 当前选中项索引。
    pub fn index(&self) -> usize {
        self.base.selected_index
    }

    fn apply_selection(&mut self, i: usize) {
        if let Some(s) = self.options.get(i) {
            self.base.selected_index = i;
            self.base.text = s.clone();
        }
    }
}

impl Default for ComboBox {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ComboBox {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        // 文字 + 左右内边距 + 右侧箭头区。
        layout::size_from_content(&self.base, s.width + 40.0, s.height + 12.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
        // 文本区留出右侧箭头宽度。
        let arrow_w = 18.0;
        let text_rect = Rect::new(
            content.left(),
            content.top(),
            (content.size.width - arrow_w).max(0.0),
            content.size.height,
        );
        draw_aligned_text(cv, &self.base.text, text_rect, &self.base.font, color, TextAlign::Left, true);
        // 右侧下拉箭头 ▼（用三角形填充）。
        let cx = content.right() - arrow_w / 2.0;
        let cy = content.top() + content.size.height / 2.0;
        let r = 4.0;
        // 以三条细横线近似三角（画布无 fill_path，用矩形叠近似）。
        for k in 0..=(r as i32) {
            let half = r - k as f32;
            let y = cy - r / 2.0 + k as f32;
            cv.fill_rect(Rect::new(cx - half, y, half * 2.0, 1.0), color);
        }
    }
    fn menu_items(&self) -> Option<Vec<String>> {
        if self.options.is_empty() {
            None
        } else {
            Some(self.options.clone())
        }
    }
    fn set_selected_item(&mut self, i: usize) {
        self.apply_selection(i);
    }
}

common_builders!(ComboBox);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combobox_选项与选择() {
        let mut c = ComboBox::new().options(["A", "B", "C"]);
        assert_eq!(c.base().text, "A", "默认回填首项");
        assert_eq!(c.menu_items(), Some(vec!["A".to_string(), "B".to_string(), "C".to_string()]));
        c.set_selected_item(2);
        assert_eq!(c.base().text, "C");
        assert_eq!(c.index(), 2);
        // 越界忽略
        c.set_selected_item(9);
        assert_eq!(c.index(), 2);
    }

    #[test]
    fn combobox_无选项不弹菜单() {
        let c = ComboBox::new();
        assert_eq!(c.menu_items(), None);
    }
}
