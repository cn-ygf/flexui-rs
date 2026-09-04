//! 统一的双轴滚动组件（L3）。
//!
//! 把「偏移 / 夹取 / 命中跟随 / 滚动条几何 + 绘制 / 动画桥接」抽成可复用的 `ScrollState`，
//! 供 Edit（单行横向、多行纵向）、ListView、ScrollView 及未来虚拟列表复用，避免各控件重复实现。
//!
//! 约定：`offset` 表示「内容相对视口向左上卷起的像素」——即绘制时子内容整体平移 `-offset`。

use flexui_gfx::{Canvas, ImageFit, ImageSource};
use flexui_gfx::{Color, Corners, Point, Rect, Size};

use crate::anim::AnimProp;
use crate::style::StyleSpec;

/// 滚动条可见性模式（对齐大厂控件的 auto/always/hidden）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollBarVisibility {
    /// 内容超出视口才显示（默认）。
    #[default]
    Auto,
    /// 始终显示（内容不足时滑块占满轨道、不可拖）。
    Always,
    /// 从不显示（仍可滚轮 / 程序滚动）。
    Hidden,
}

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
    visibility: ScrollBarVisibility,
}

impl ScrollState {
    /// 按允许的轴创建（初始偏移 0，可见性 Auto）。
    pub fn new(axes: ScrollAxes) -> Self {
        Self {
            offset: Point::new(0.0, 0.0),
            content: Size::default(),
            viewport: Size::default(),
            axes,
            visibility: ScrollBarVisibility::Auto,
        }
    }

    /// 设置滚动条可见性模式。
    pub fn set_visibility(&mut self, visibility: ScrollBarVisibility) {
        self.visibility = visibility;
    }
    pub fn visibility(&self) -> ScrollBarVisibility {
        self.visibility
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

    /// 内容是否超出视口（该轴允许时）——决定「是否可滚动」，与可见性无关。
    pub fn needs_v(&self) -> bool {
        self.axes.y && self.content.height > self.viewport.height + 0.5
    }
    pub fn needs_h(&self) -> bool {
        self.axes.x && self.content.width > self.viewport.width + 0.5
    }

    /// 是否应绘制纵向 / 横向滚动条（含可见性模式：Always 始终、Hidden 从不、Auto 看溢出）。
    pub fn show_v(&self) -> bool {
        match self.visibility {
            ScrollBarVisibility::Hidden => false,
            ScrollBarVisibility::Always => self.axes.y,
            ScrollBarVisibility::Auto => self.needs_v(),
        }
    }
    pub fn show_h(&self) -> bool {
        match self.visibility {
            ScrollBarVisibility::Hidden => false,
            ScrollBarVisibility::Always => self.axes.x,
            ScrollBarVisibility::Auto => self.needs_h(),
        }
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
    /// 滚动条与视口边缘（右缘 / 底缘）之间保留的留白，一般 1~2px。
    pub margin: f32,
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
            margin: 2.0,
            min_thumb_height: 24.0,
            thumb_color: Color::from_u8(200, 210, 230, 160),
            thumb_image: None,
            thumb_fit: ImageFit::Stretch,
        }
    }
}

/// 滚动轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

/// 抓住滚动条滑块拖动时记录的信息：哪根轴 + 鼠标按下点到滑块起始边的距离。
#[derive(Debug, Clone, Copy)]
pub struct ScrollGrab {
    pub axis: ScrollAxis,
    /// 鼠标按下点相对滑块起始边（纵条顶 / 横条左）的偏移，拖动时保持不变。
    pub grab: f32,
}

/// 命中滚动条滑块则返回抓取信息（供拖动）。`viewport`/`style` 须与绘制时一致。
pub fn thumb_grab(
    state: &ScrollState,
    viewport: Rect,
    style: &ScrollBarStyle,
    pos: Point,
) -> Option<ScrollGrab> {
    if let Some(thumb) = thumb_v(state, viewport, style) {
        if thumb.contains(pos) {
            return Some(ScrollGrab {
                axis: ScrollAxis::Vertical,
                grab: pos.y - thumb.top(),
            });
        }
    }
    if let Some(thumb) = thumb_h_rect(state, viewport, style) {
        if thumb.contains(pos) {
            return Some(ScrollGrab {
                axis: ScrollAxis::Horizontal,
                grab: pos.x - thumb.left(),
            });
        }
    }
    None
}

/// 按拖动中的鼠标位置反推并设置滚动偏移。返回是否变化。
pub fn apply_thumb_drag(
    state: &mut ScrollState,
    viewport: Rect,
    style: &ScrollBarStyle,
    pos: Point,
    grab: &ScrollGrab,
) -> bool {
    match grab.axis {
        ScrollAxis::Vertical => {
            let Some(thumb) = thumb_v(state, viewport, style) else {
                return false;
            };
            let travel = viewport.size.height - thumb.size.height;
            if travel <= 0.0 {
                return false;
            }
            let t = ((pos.y - grab.grab - viewport.top()) / travel).clamp(0.0, 1.0);
            state.set_offset(state.offset().x, t * state.max().y)
        }
        ScrollAxis::Horizontal => {
            let Some(thumb) = thumb_h_rect(state, viewport, style) else {
                return false;
            };
            let travel = viewport.size.width - thumb.size.width;
            if travel <= 0.0 {
                return false;
            }
            let t = ((pos.x - grab.grab - viewport.left()) / travel).clamp(0.0, 1.0);
            state.set_offset(t * state.max().x, state.offset().y)
        }
    }
}

/// 纵向滑块矩形（不显示则 None）。`viewport` 为视口矩形（绝对坐标）。
pub fn thumb_v(state: &ScrollState, viewport: Rect, style: &ScrollBarStyle) -> Option<Rect> {
    if !state.show_v() {
        return None;
    }
    let vh = viewport.size.height;
    let content_h = state.content().height.max(vh); // Always 且内容不足时滑块占满
    let ratio = vh / content_h;
    let thumb_h = (vh * ratio).max(style.min_thumb_height).min(vh);
    let max = (content_h - vh).max(1.0);
    let t = (state.offset().y / max).clamp(0.0, 1.0);
    let y = viewport.top() + (vh - thumb_h) * t;
    // 贴右缘，留 margin 留白。
    let x = viewport.right() - style.width - style.margin;
    Some(Rect::new(x, y, style.width, thumb_h))
}

/// 横向滑块矩形（不显示则 None）。
pub fn thumb_h_rect(state: &ScrollState, viewport: Rect, style: &ScrollBarStyle) -> Option<Rect> {
    if !state.show_h() {
        return None;
    }
    let vw = viewport.size.width;
    let content_w = state.content().width.max(vw);
    let ratio = vw / content_w;
    let thumb_w = (vw * ratio).max(style.min_thumb_height).min(vw);
    let max = (content_w - vw).max(1.0);
    let t = (state.offset().x / max).clamp(0.0, 1.0);
    let x = viewport.left() + (vw - thumb_w) * t;
    // 贴底缘，留 margin 留白。
    let y = viewport.bottom() - style.width - style.margin;
    Some(Rect::new(x, y, thumb_w, style.width))
}

/// 该点是否落在滚动条区域（滑块所在的整条轨道，用于光标形状 / 命中判断）。
pub fn scrollbar_region_contains(
    state: &ScrollState,
    viewport: Rect,
    style: &ScrollBarStyle,
    pos: Point,
) -> bool {
    if state.show_v() {
        let x = viewport.right() - style.width - style.margin;
        let track = Rect::new(x, viewport.top(), style.width, viewport.size.height);
        if track.contains(pos) {
            return true;
        }
    }
    if state.show_h() {
        let y = viewport.bottom() - style.width - style.margin;
        let track = Rect::new(viewport.left(), y, viewport.size.width, style.width);
        if track.contains(pos) {
            return true;
        }
    }
    false
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
    fn 拖动滑块反推偏移() {
        let mut s = ScrollState::new(ScrollAxes::vertical());
        s.set_metrics(Size::new(100.0, 300.0), Size::new(100.0, 100.0));
        let vp = Rect::new(0.0, 0.0, 100.0, 100.0);
        let style = ScrollBarStyle::default();
        // 滑块在右缘(宽5、边距2)→ x∈[93,98]，初始 y∈[0,33.33]。
        let grab = thumb_grab(&s, vp, &style, Point::new(95.0, 10.0)).expect("应命中滑块");
        assert_eq!(grab.axis, ScrollAxis::Vertical);
        // 抓取点距滑块顶 10；拖到 y=60 → 滑块顶=50，行程=100-33.33=66.67，t≈0.75。
        assert!(apply_thumb_drag(
            &mut s,
            vp,
            &style,
            Point::new(95.0, 60.0),
            &grab
        ));
        assert!(
            (s.offset().y - 150.0).abs() < 1.0,
            "offset={}",
            s.offset().y
        );
        // 滑块外的点不命中。
        assert!(thumb_grab(&s, vp, &style, Point::new(10.0, 10.0)).is_none());
    }

    #[test]
    fn 可见性模式() {
        let style = ScrollBarStyle::default();
        let vp = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut s = ScrollState::new(ScrollAxes::vertical());
        // 内容不足视口：Auto 不显示。
        s.set_metrics(Size::new(100.0, 50.0), Size::new(100.0, 100.0));
        assert!(!s.show_v());
        assert!(thumb_v(&s, vp, &style).is_none());
        // Always：即使不足也显示，滑块占满。
        s.set_visibility(ScrollBarVisibility::Always);
        assert!(s.show_v());
        let thumb = thumb_v(&s, vp, &style).expect("Always 应有滑块");
        assert_eq!(thumb.size.height, 100.0);
        // Hidden：即使溢出也不显示，但仍可滚动（needs_v）。
        s.set_visibility(ScrollBarVisibility::Hidden);
        s.set_metrics(Size::new(100.0, 300.0), Size::new(100.0, 100.0));
        assert!(!s.show_v());
        assert!(s.needs_v());
        assert!(thumb_v(&s, vp, &style).is_none());
        // 落在滚动条区域判断随可见性变化。
        s.set_visibility(ScrollBarVisibility::Auto);
        assert!(scrollbar_region_contains(
            &s,
            vp,
            &style,
            Point::new(95.0, 50.0)
        ));
        assert!(!scrollbar_region_contains(
            &s,
            vp,
            &style,
            Point::new(10.0, 50.0)
        ));
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
