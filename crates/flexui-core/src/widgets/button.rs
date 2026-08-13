//! Button：按钮控件（完整 4×2 状态，点击触发回调/通知）。

use flexui_geometry::{Color, Rect, Size};
use flexui_gfx::{Canvas, ImageFit, ImageSource, TextAlign};

use crate::common_builders;
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{
    Base, Clickable, TextControl, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole,
};

/// 按钮：完整 4×2 状态，点击触发回调。
pub struct Button {
    base: Base,
    icon: Option<ImageSource>,
    /// 相对按钮左上角的图标矩形；未设置时在内容区居中绘制 18x18 图标。
    icon_rect: Option<Rect>,
    /// 相对按钮左上角的文本矩形；未设置时使用按钮内容区。
    text_rect: Option<Rect>,
}

impl Button {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new_kind(WidgetRole::Button, WidgetKind::Button);
        base.text = text.into();
        Self {
            base,
            icon: None,
            icon_rect: None,
            text_rect: None,
        }
    }

    /// 设置按钮图标。
    pub fn icon(mut self, icon: ImageSource) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 设置相对按钮左上角的图标绘制矩形。
    pub fn icon_rect(mut self, rect: Rect) -> Self {
        self.icon_rect = Some(rect);
        self
    }

    /// 设置相对按钮左上角的文本绘制矩形。
    pub fn text_rect(mut self, rect: Rect) -> Self {
        self.text_rect = Some(rect);
        self
    }
}

impl Widget for Button {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        let icon_width = self.icon.as_ref().map_or(0.0, |_| {
            self.icon_rect.map_or(18.0, |rect| rect.right().max(0.0))
        });
        let text_width = self.text_rect.map_or(s.width, |rect| rect.right().max(0.0));
        // 未指定绘制区域时，图标与文字并排参与内容尺寸；显式区域按最远右边界计量。
        let content_width = if self.icon_rect.is_some() || self.text_rect.is_some() {
            icon_width.max(text_width)
        } else if self.icon.is_some() && !self.base.text.is_empty() {
            icon_width + 8.0 + s.width
        } else {
            icon_width.max(s.width)
        };
        layout::size_from_content(&self.base, content_width + 24.0, s.height.max(18.0) + 12.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        let automatic_pair = self.icon.is_some()
            && !self.base.text.is_empty()
            && self.icon_rect.is_none()
            && self.text_rect.is_none();
        let text_size = automatic_pair.then(|| cv.measure_text(&self.base.text, &self.base.font));
        if let Some(icon) = &self.icon {
            let icon_rect = self.icon_rect.map_or_else(
                || {
                    let x = text_size.as_ref().map_or_else(
                        || content.left() + (content.size.width - 18.0) / 2.0,
                        |text| {
                            content.left() + (content.size.width - (18.0 + 8.0 + text.width)) / 2.0
                        },
                    );
                    Rect::new(
                        x,
                        content.top() + (content.size.height - 18.0) / 2.0,
                        18.0,
                        18.0,
                    )
                },
                |rect| absolute_rect(self.base.rect, rect),
            );
            cv.draw_image(icon, icon_rect, style.fg_tint, ImageFit::Stretch);
        }
        let color = style.fg_color.unwrap_or(Color::WHITE);
        let align = style.text_align.unwrap_or(TextAlign::Center);
        let text_content = self.text_rect.map_or_else(
            || {
                text_size.as_ref().map_or(content, |text| {
                    let group_left =
                        content.left() + (content.size.width - (18.0 + 8.0 + text.width)) / 2.0;
                    Rect::new(
                        group_left + 26.0,
                        content.top(),
                        text.width,
                        content.size.height,
                    )
                })
            },
            |rect| absolute_rect(self.base.rect, rect),
        );
        draw_aligned_text(
            cv,
            &self.base.text,
            text_content,
            &self.base.font,
            color,
            align,
            true,
        );
    }
    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Icon(icon) => self.icon = icon,
            WidgetProperty::IconRect(rect) => self.icon_rect = rect,
            WidgetProperty::TextRect(rect) => self.text_rect = rect,
            _ => return false,
        }
        true
    }
    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        match key {
            WidgetPropertyKey::Icon => Some(WidgetProperty::Icon(self.icon.clone())),
            WidgetPropertyKey::IconRect => Some(WidgetProperty::IconRect(self.icon_rect)),
            WidgetPropertyKey::TextRect => Some(WidgetProperty::TextRect(self.text_rect)),
            _ => None,
        }
    }
}

fn absolute_rect(button: Rect, relative: Rect) -> Rect {
    Rect::new(
        button.left() + relative.left(),
        button.top() + relative.top(),
        relative.size.width,
        relative.size.height,
    )
}

common_builders!(Button);

impl TextControl for Button {}
impl Clickable for Button {}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui_geometry::{Corners, Point};
    use flexui_gfx::Font;

    #[derive(Default)]
    struct Recorder {
        images: Vec<Rect>,
        text_origins: Vec<Point>,
    }

    impl Canvas for Recorder {
        fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: Color, _line_width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: Color) {}
        fn stroke_round_rect(
            &mut self,
            _rect: Rect,
            _radius: Corners,
            _color: Color,
            _line_width: f32,
        ) {
        }
        fn draw_text(&mut self, _text: &str, origin: Point, _font: &Font, _color: Color) {
            self.text_origins.push(origin);
        }
        fn measure_text(&self, text: &str, font: &Font) -> Size {
            Size::new(text.len() as f32 * font.size * 0.5, font.size)
        }
        fn draw_image(
            &mut self,
            _source: &ImageSource,
            rect: Rect,
            _tint: Option<Color>,
            _fit: ImageFit,
        ) {
            self.images.push(rect);
        }
    }

    #[test]
    fn 图标和文本可绘制到相对按钮的指定矩形() {
        let mut button = Button::new("Open")
            .icon(ImageSource::path("open.png"))
            .icon_rect(Rect::new(12.0, 10.0, 20.0, 20.0))
            .text_rect(Rect::new(44.0, 0.0, 80.0, 40.0));
        button.base.rect = Rect::new(100.0, 50.0, 140.0, 40.0);
        let mut recorder = Recorder::default();
        button.paint_content(&mut recorder, &StyleSpec::default());
        assert_eq!(recorder.images, vec![Rect::new(112.0, 60.0, 20.0, 20.0)]);
        assert!(recorder.text_origins[0].x >= 144.0);
    }

    #[test]
    fn 图标相关属性支持运行时对称读写() {
        let mut button = Button::new("");
        let rect = Rect::new(8.0, 8.0, 16.0, 16.0);
        assert!(button.apply_property(WidgetProperty::Icon(Some(ImageSource::path("icon.png")))));
        assert!(button.apply_property(WidgetProperty::IconRect(Some(rect))));
        assert!(matches!(
            button.property(WidgetPropertyKey::IconRect),
            Some(WidgetProperty::IconRect(Some(value))) if value == rect
        ));
    }

    #[test]
    fn 未指定矩形时图标和文本作为整体居中且不重叠() {
        let mut button = Button::new("Open").icon(ImageSource::path("open.png"));
        button.base.rect = Rect::new(0.0, 0.0, 120.0, 40.0);
        let mut recorder = Recorder::default();
        button.paint_content(&mut recorder, &StyleSpec::default());
        assert!(recorder.images[0].right() + 8.0 <= recorder.text_origins[0].x);
    }
}
