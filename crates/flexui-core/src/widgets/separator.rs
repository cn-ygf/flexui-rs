//! Separator：分隔条（细线，横向或纵向）。

use flexui_gfx::{Color, Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout;
use crate::sizing::Sizing;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole};

/// 分隔条：默认横向（占满宽、细高）；纵向则占满高、细宽。线色取 bg_color 或默认灰。
pub struct Separator {
    base: Base,
    vertical: bool,
    thickness: f32,
}

impl Separator {
    pub fn new() -> Self {
        let mut base = Base::new_kind(WidgetRole::Plain, WidgetKind::Separator);
        base.hit = crate::widget::HitPolicy::Transparent; // 分隔条不拦截命中
        base.width = Sizing::Fill;
        base.height = Sizing::Fixed(1.0);
        Self {
            base,
            vertical: false,
            thickness: 1.0,
        }
    }
    /// 设为纵向分隔条。
    pub fn vertical(mut self, on: bool) -> Self {
        self.set_vertical(on);
        self
    }
    /// 线粗（逻辑像素）。
    pub fn thickness(mut self, t: f32) -> Self {
        self.set_thickness(t);
        self
    }
    fn set_vertical(&mut self, on: bool) {
        self.vertical = on;
        if on {
            self.base.width = Sizing::Fixed(self.thickness);
            self.base.height = Sizing::Fill;
        } else {
            self.base.width = Sizing::Fill;
            self.base.height = Sizing::Fixed(self.thickness);
        }
    }
    fn set_thickness(&mut self, t: f32) {
        self.thickness = t.max(1.0);
        if self.vertical {
            self.base.width = Sizing::Fixed(self.thickness);
        } else {
            self.base.height = Sizing::Fixed(self.thickness);
        }
    }
}

impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Separator {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        if self.vertical {
            layout::size_from_content(&self.base, self.thickness, 1.0)
        } else {
            layout::size_from_content(&self.base, 1.0, self.thickness)
        }
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        let color = style.bg_color.unwrap_or(Color::from_u8(74, 80, 100, 255));
        if content.size.width > 0.0 && content.size.height > 0.0 {
            cv.fill_rect(
                Rect::new(
                    content.left(),
                    content.top(),
                    content.size.width,
                    content.size.height,
                ),
                color,
            );
        }
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Vertical(vertical) => {
                self.set_vertical(vertical);
                true
            }
            WidgetProperty::Thickness(thickness) => {
                self.set_thickness(thickness);
                true
            }
            _ => false,
        }
    }
    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        match key {
            WidgetPropertyKey::Vertical => Some(WidgetProperty::Vertical(self.vertical)),
            WidgetPropertyKey::Thickness => Some(WidgetProperty::Thickness(self.thickness)),
            _ => None,
        }
    }
}

common_builders!(Separator);
