//! Image：图片控件。

use flexui_geometry::Size;
use flexui_gfx::{Canvas, ImageSource};

use crate::common_builders;
use crate::layout;
use crate::style::StyleSpec;
use crate::widget::{Base, Widget, WidgetRole};

/// 图片控件。
pub struct Image {
    base: Base,
    source: ImageSource,
}

impl Image {
    pub fn new(source: ImageSource) -> Self {
        Self {
            base: Base::new(WidgetRole::Plain),
            source,
        }
    }
    pub fn path(p: impl Into<String>) -> Self {
        Self::new(ImageSource::path(p))
    }
}

impl Widget for Image {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        // 图片无内在尺寸信息时用显式尺寸，缺省给一个占位方块。
        layout::size_from_content(&self.base, 32.0, 32.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, _style: &StyleSpec) {
        cv.draw_image(&self.source, layout::content_rect(&self.base));
    }
}

common_builders!(Image);
