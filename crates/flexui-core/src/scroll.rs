//! 统一的双轴滚动组件（L3）。
//!
//! 把「偏移 / 夹取 / 命中跟随 / 滚动条几何 + 绘制 / 动画桥接」抽成可复用的 `ScrollState`，
//! 供 Edit（单行横向、多行纵向）、ListView、ScrollView 及未来虚拟列表复用，避免各控件重复实现。
//!
//! 约定：`offset` 表示「内容相对视口向左上卷起的像素」——即绘制时子内容整体平移 `-offset`。

use flexui_geometry::{Color, Corners, Point, Rect, Size};
use flexui_gfx::{Canvas, ImageFit, ImageSource};

use crate::anim::AnimProp;
use crate::style::StyleSpec;

/// 允许滚动的轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollAxes {
    pub x: bool,
    pub y: bool,
}

impl ScrollAxes {
    pub const fn new(x: bool, y: bool) -> Self {
        Self { x, y }
    }
    /// 仅横向。
    pub const fn horizontal() -> Self {
        Self::new(true, false)
    }
    /// 仅纵向。
    pub const fn vertical() -> Self {
        Self::new(false, true)
    }
    /// 两轴。
    pub const fn both() -> Self {
        Self::new(true, true)
    }
}

/// 双轴滚动状态：记录当前偏移、内容尺寸、视口尺寸与允许的轴，负责夹取与命中跟随。
#[derive(Debug, Clone, Copy)]
pub struct ScrollState {
    offset: Point,
    content: Size,
    viewport: Size,
    axes: ScrollAxes,
}

impl ScrollState {
    /// 按允许的轴创建（初始偏移 0）。
    pub fn new(axes: ScrollAxes) -> Self {
        Self {
            offset: Point::new(0.0, 0.0),
            content: Size::default(),
            viewport: Size::default(),
            axes,
        }
    }

    /// 更新内容与视口尺寸（一般在 arrange 时调用），随即把偏移夹到合法范围。
    pub fn set_metrics(&mut self, content: Size, viewport: Size) {
        self.content = content;
        self.viewport = viewport;
        self.clamp();
    }

    /// 当前偏移。
    pub fn offset(&self) -> Point {
        self.offset
    }
    pub fn content(&self) -> Size {
        self.content
    }
    pub fn viewport(&self) -> Size {
        self.viewport
    }
    pub fn axes(&self) -> ScrollAxes {
        self.axes
    }

    /// 各轴最大偏移（内容不足视口时为 0）。
    pub fn max(&self) -> Point {
        Point::new(
            if self.axes.x {
                (self.content.width - self.viewport.width).max(0.0)
            } else {
                0.0
            },
            if self.axes.y {
                (self.content.height - self.viewport.height).max(0.0)
            } else {
                0.0
            },
        )
    }

    fn clamp(&mut self) {
        let m = self.max();
        self.offset.x = self.offset.x.clamp(0.0, m.x);
        self.offset.y = self.offset.y.clamp(0.0, m.y);
    }

    /// 直接设置偏移（自动夹取）。返回是否变化。
    pub fn set_offset(&mut self, x: f32, y: f32) -> bool {
        let m = self.max();
        let nx = if self.axes.x { x.clamp(0.0, m.x) } else { 0.0 };
        let ny = if self.axes.y { y.clamp(0.0, m.y) } else { 0.0 };
        let changed = nx != self.offset.x || ny != self.offset.y;
        self.offset = Point::new(nx, ny);
        changed
    }

    /// 滚轮/拖动增量滚动：`dy>0` 通常表示内容向上滚（偏移增大）。返回是否变化。
    ///
    /// 语义与旧实现一致（`scroll_by(dy)` 时偏移 = 旧偏移 - dy），这里保留：正 dy 减小偏移。
    pub fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.set_offset(self.offset.x - dx, self.offset.y - dy)
    }

    /// 需要纵向 / 横向滚动条（内容超出视口且该轴允许）。
    pub fn needs_v(&self) -> bool {
        self.axes.y && self.content.height > self.viewport.height + 0.5
    }
    pub fn needs_h(&self) -> bool {
        self.axes.x && self.content.width > self.viewport.width + 0.5
    }

    /// 调整偏移使内容坐标下的矩形 `rect` 落入视口（光标 / 选中行跟随）。返回是否变化。
    ///
    /// `rect` 以内容左上角为原点（不含当前偏移）。`pad` 为额外留白。
    pub fn ensure_visible(&mut self, rect: Rect, pad: f32) -> bool {
        let mut off = self.offset;
        if self.axes.x {
            let vw = self.viewport.width;
            if rect.left() - pad < off.x {
                off.x = rect.left() - pad;
            } else if rect.right() + pad > off.x + vw {
                off.x = rect.right() + pad - vw;
            }
        }
        if self.axes.y {
            let vh = self.viewport.height;
            if rect.top() - pad < off.y {
                off.y = rect.top() - pad;
            } else if rect.bottom() + pad > off.y + vh {
                off.y = rect.bottom() + pad - vh;
            }
        }
        self.set_offset(off.x, off.y)
    }

    /// 动画桥接：读某轴当前值。
    pub fn axis_value(&self, prop: AnimProp) -> Option<f32> {
        match prop {
            AnimProp::ScrollX if self.axes.x => Some(self.offset.x),
            AnimProp::ScrollY if self.axes.y => Some(self.offset.y),
            _ => None,
        }
    }
    /// 动画桥接：设某轴值（夹取）。返回是否处理了该属性。
    pub fn set_axis_value(&mut self, prop: AnimProp, value: f32) -> bool {
        match prop {
            AnimProp::ScrollX if self.axes.x => {
                self.set_offset(value, self.offset.y);
                true
            }
            AnimProp::ScrollY if self.axes.y => {
                self.set_offset(self.offset.x, value);
                true
            }
            _ => false,
        }
    }
}

/// 滚动条外观（两轴共用）。
#[derive(Debug, Clone)]
pub struct ScrollBarStyle {
    /// 条粗（纵条的宽 / 横条的高）。
    pub width: f32,
    /// 内容与滚动条之间保留的间距。
    pub gap: f32,
    /// 滑块最小长度（沿滚动轴）。
    pub min_thumb_height: f32,
    pub thumb_color: Color,
    pub thumb_image: Option<ImageSource>,
    pub thumb_fit: ImageFit,
}

impl Default for ScrollBarStyle {
    fn default() -> Self {
        Self {
            width: 5.0,
            gap: 4.0,
            min_thumb_height: 24.0,
            thumb_color: Color::from_u8(200, 210, 230, 160),
            thumb_image: None,
            thumb_fit: ImageFit::Stretch,
        }
    }
}

/// 纵向滑块矩形（内容不足则 None）。`viewport` 为视口矩形（绝对坐标）。
pub fn thumb_v(state: &ScrollState, viewport: Rect, style: &ScrollBarStyle) -> Option<Rect> {
    if !state.needs_v() {
        return None;
    }
    let vh = viewport.size.height;
    let content_h = state.content().height;
    let ratio = vh / content_h;
    let thumb_h = (vh * ratio).max(style.min_thumb_height).min(vh);
    let max = (content_h - vh).max(1.0);
    let t = (state.offset().y / max).clamp(0.0, 1.0);
    let y = viewport.top() + (vh - thumb_h) * t;
    Some(Rect::new(viewport.right() - style.width, y, style.width, thumb_h))
}

/// 横向滑块矩形（内容不足则 None）。
pub fn thumb_h_rect(state: &ScrollState, viewport: Rect, style: &ScrollBarStyle) -> Option<Rect> {
    if !state.needs_h() {
        return None;
    }
    let vw = viewport.size.width;
    let content_w = state.content().width;
    let ratio = vw / content_w;
    let thumb_w = (vw * ratio).max(style.min_thumb_height).min(vw);
    let max = (content_w - vw).max(1.0);
    let t = (state.offset().x / max).clamp(0.0, 1.0);
    let x = viewport.left() + (vw - thumb_w) * t;
    Some(Rect::new(x, viewport.bottom() - style.width, thumb_w, style.width))
}

/// 按需绘制纵/横滚动条到视口之上（供 ScrollView/ListView/Edit 复用）。
pub fn paint_scrollbars(
    cv: &mut dyn Canvas,
    viewport: Rect,
    state: &ScrollState,
    style: &ScrollBarStyle,
    spec: &StyleSpec,
) {
    for thumb in [
        thumb_v(state, viewport, style),
        thumb_h_rect(state, viewport, style),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(image) = &style.thumb_image {
            cv.draw_image(image, thumb, None, style.thumb_fit.clone());
        } else {
            let r = style.width / 2.0;
            cv.fill_round_rect(
                thumb,
                Corners::all(r),
                spec.scrollbar_color.unwrap_or(style.thumb_color),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_by_双轴夹取() {
        let mut s = ScrollState::new(ScrollAxes::both());
        s.set_metrics(Size::new(300.0, 300.0), Size::new(100.0, 100.0));
        assert_eq!(s.max(), Point::new(200.0, 200.0));
        // 正 dy 减小偏移；从 0 往负方向被夹到 0。
        assert!(!s.scroll_by(0.0, 10.0));
        // 负 dy 增大偏移。
        assert!(s.scroll_by(0.0, -60.0));
        assert_eq!(s.offset().y, 60.0);
        assert!(s.scroll_by(-30.0, 0.0));
        assert_eq!(s.offset().x, 30.0);
        // 越界夹取。
        s.scroll_by(-999.0, -999.0);
        assert_eq!(s.offset(), Point::new(200.0, 200.0));
    }

    #[test]
    fn 仅纵轴时横向不动() {
        let mut s = ScrollState::new(ScrollAxes::vertical());
        s.set_metrics(Size::new(300.0, 300.0), Size::new(100.0, 100.0));
        assert!(!s.scroll_by(-50.0, 0.0));
        assert_eq!(s.offset().x, 0.0);
    }

    #[test]
    fn ensure_visible_滚到可见() {
        let mut s = ScrollState::new(ScrollAxes::vertical());
        s.set_metrics(Size::new(100.0, 500.0), Size::new(100.0, 100.0));
        // 目标行在 y=250..270，视口高 100 → 偏移应使其底部进入视口。
        assert!(s.ensure_visible(Rect::new(0.0, 250.0, 100.0, 20.0), 0.0));
        assert_eq!(s.offset().y, 170.0); // 270 - 100
        // 已可见则不动。
        assert!(!s.ensure_visible(Rect::new(0.0, 250.0, 100.0, 20.0), 0.0));
        // 目标在上方 → 顶部对齐。
        assert!(s.ensure_visible(Rect::new(0.0, 40.0, 100.0, 20.0), 0.0));
        assert_eq!(s.offset().y, 40.0);
    }

    #[test]
    fn 动画桥接() {
        let mut s = ScrollState::new(ScrollAxes::vertical());
        s.set_metrics(Size::new(100.0, 300.0), Size::new(100.0, 100.0));
        assert_eq!(s.axis_value(AnimProp::ScrollY), Some(0.0));
        assert_eq!(s.axis_value(AnimProp::ScrollX), None);
        assert!(s.set_axis_value(AnimProp::ScrollY, 150.0));
        assert_eq!(s.offset().y, 150.0);
        assert!(!s.set_axis_value(AnimProp::ScrollX, 10.0));
    }
}
