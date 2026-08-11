//! Progress：进度条（按 `Base.value` 归一化 0~1 显示进度）。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::Canvas;

use crate::common_builders;
use crate::layout;
use crate::sizing::Sizing;
use crate::style::StyleSpec;
use crate::widget::{Base, Widget, WidgetRole};

/// 进度条：轨道用 bg_color（或默认灰），进度用 fg_color（或默认蓝），圆角胶囊。
pub struct Progress {
    base: Base,
}

impl Progress {
    pub fn new() -> Self {
        let mut base = Base::new(WidgetRole::Plain);
        base.width = Sizing::Fill;
        base.height = Sizing::Fixed(8.0);
        Self { base }
    }
    /// 设置进度（0~1，自动夹取）。
    pub fn value(mut self, v: f32) -> Self {
        self.base.value = v.clamp(0.0, 1.0);
        self
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Progress {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        layout::size_from_content(&self.base, 120.0, 8.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        if content.size.width <= 0.0 || content.size.height <= 0.0 {
            return;
        }
        let track = style.bg_color.unwrap_or(Color::from_u8(60, 64, 74, 255));
        let fill = style.fg_color.unwrap_or(Color::from_u8(52, 120, 246, 255));
        let radius = Corners::all(content.size.height / 2.0);
        cv.fill_round_rect(content, radius, track);
        let v = self.base.value.clamp(0.0, 1.0);
        let w = content.size.width * v;
        if w > 0.0 {
            let fill_rect = Rect::new(content.left(), content.top(), w, content.size.height);
            cv.fill_round_rect(fill_rect, radius, fill);
        }
    }
}

common_builders!(Progress);
