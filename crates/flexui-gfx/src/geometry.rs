//! 平台无关的几何与颜色基础类型。
//!
//! 统一使用「逻辑像素、左上原点、y 向下」的坐标系；平台后端负责把这里的类型
//! 转换成各自的原生类型（如 macOS 的 NSRect），上层控件通过 `flexui-gfx` 使用。

/// 二维点。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 尺寸（宽高）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// 二维仿射变换。
///
/// 点按 `x' = m11*x + m21*y + dx`、`y' = m12*x + m22*y + dy` 变换。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Affine {
    pub const IDENTITY: Self = Self {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 0.0,
        dy: 0.0,
    };

    pub const fn new(m11: f32, m12: f32, m21: f32, m22: f32, dx: f32, dy: f32) -> Self {
        Self {
            m11,
            m12,
            m21,
            m22,
            dx,
            dy,
        }
    }

    pub const fn translation(x: f32, y: f32) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    pub const fn scale(x: f32, y: f32) -> Self {
        Self::new(x, 0.0, 0.0, y, 0.0, 0.0)
    }

    pub fn rotation(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::new(cos, sin, -sin, cos, 0.0, 0.0)
    }

    /// 先应用 `self`，再应用 `next`。
    pub fn then(self, next: Self) -> Self {
        Self::new(
            next.m11 * self.m11 + next.m21 * self.m12,
            next.m12 * self.m11 + next.m22 * self.m12,
            next.m11 * self.m21 + next.m21 * self.m22,
            next.m12 * self.m21 + next.m22 * self.m22,
            next.m11 * self.dx + next.m21 * self.dy + next.dx,
            next.m12 * self.dx + next.m22 * self.dy + next.dy,
        )
    }

    pub fn transform_point(self, point: Point) -> Point {
        Point::new(
            self.m11 * point.x + self.m21 * point.y + self.dx,
            self.m12 * point.x + self.m22 * point.y + self.dy,
        )
    }

    pub fn transform_vector(self, vector: Point) -> Point {
        Point::new(
            self.m11 * vector.x + self.m21 * vector.y,
            self.m12 * vector.x + self.m22 * vector.y,
        )
    }

    pub fn transform_rect(self, rect: Rect) -> Rect {
        let points = [
            self.transform_point(Point::new(rect.left(), rect.top())),
            self.transform_point(Point::new(rect.right(), rect.top())),
            self.transform_point(Point::new(rect.right(), rect.bottom())),
            self.transform_point(Point::new(rect.left(), rect.bottom())),
        ];
        let left = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let top = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let right = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        let bottom = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        Rect::new(left, top, right - left, bottom - top)
    }

    pub fn inverse(self) -> Option<Self> {
        let determinant = self.m11 * self.m22 - self.m12 * self.m21;
        if !determinant.is_finite() || determinant.abs() <= f32::EPSILON {
            return None;
        }
        let inv = 1.0 / determinant;
        let m11 = self.m22 * inv;
        let m12 = -self.m12 * inv;
        let m21 = -self.m21 * inv;
        let m22 = self.m11 * inv;
        Some(Self::new(
            m11,
            m12,
            m21,
            m22,
            -(m11 * self.dx + m21 * self.dy),
            -(m12 * self.dx + m22 * self.dy),
        ))
    }

    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// 矩形：左上角 origin + 尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    pub fn left(&self) -> f32 {
        self.origin.x
    }
    pub fn top(&self) -> f32 {
        self.origin.y
    }
    pub fn right(&self) -> f32 {
        self.origin.x + self.size.width
    }
    pub fn bottom(&self) -> f32 {
        self.origin.y + self.size.height
    }

    /// 点是否落在矩形内（用于命中测试）。
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left() && p.x < self.right() && p.y >= self.top() && p.y < self.bottom()
    }

    /// 向内收缩（每边减去对应内边距），用于从边框/内边距推出内容区。
    pub fn deflate(&self, insets: Insets) -> Rect {
        let w = (self.size.width - insets.left - insets.right).max(0.0);
        let h = (self.size.height - insets.top - insets.bottom).max(0.0);
        Rect::new(
            self.origin.x + insets.left,
            self.origin.y + insets.top,
            w,
            h,
        )
    }

    /// 四边收缩相同值。
    pub fn inset_all(&self, v: f32) -> Rect {
        self.deflate(Insets::all(v))
    }
}

/// 把居中描边转换为整数物理像素宽度，并返回位于原矩形内部的中心路径。
///
/// 返回的线宽仍使用逻辑像素；调用方施加相同的 DPI 缩放后，边框外沿会落在
/// 物理像素边界上，避免细线跨像素产生灰色颗粒。矩形过小或参数无效时返回 None。
pub fn pixel_aligned_stroke(rect: Rect, line_width: f32, scale: f32) -> Option<(Rect, f32)> {
    if !line_width.is_finite()
        || line_width <= 0.0
        || !scale.is_finite()
        || scale <= 0.0
        || !rect.origin.x.is_finite()
        || !rect.origin.y.is_finite()
        || !rect.size.width.is_finite()
        || !rect.size.height.is_finite()
        || rect.size.width <= 0.0
        || rect.size.height <= 0.0
    {
        return None;
    }

    let physical_width = (line_width * scale).round().max(1.0);
    let left = (rect.left() * scale).ceil();
    let top = (rect.top() * scale).ceil();
    let right = (rect.right() * scale).floor();
    let bottom = (rect.bottom() * scale).floor();
    let inset = physical_width / 2.0;
    if right - left < physical_width || bottom - top < physical_width {
        return None;
    }

    let path_left = left + inset;
    let path_top = top + inset;
    let path_right = right - inset;
    let path_bottom = bottom - inset;
    Some((
        Rect::new(
            path_left / scale,
            path_top / scale,
            (path_right - path_left) / scale,
            (path_bottom - path_top) / scale,
        ),
        physical_width / scale,
    ))
}

/// 四边内边距/外框厚度。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Insets {
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn all(v: f32) -> Self {
        Self::new(v, v, v, v)
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// RGBA 颜色，分量范围 0.0~1.0。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// 由 0~255 整数构造，便于对齐设计稿。
    pub fn from_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
}

/// 四角圆角半径（对齐需求：圆角参数可四角独立）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Corners {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

impl Corners {
    /// 四角相同半径。
    pub const fn all(r: f32) -> Self {
        Self {
            tl: r,
            tr: r,
            br: r,
            bl: r,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }

    #[test]
    fn 单像素描边中心落在半像素位置() {
        let (path, width) =
            pixel_aligned_stroke(Rect::new(0.0, 0.0, 100.0, 40.0), 1.0, 1.0).unwrap();
        assert_eq!(path, Rect::new(0.5, 0.5, 99.0, 39.0));
        assert_eq!(width, 1.0);
    }

    #[test]
    fn 分数_dpi_将线宽吸附到整数物理像素() {
        let (path_125, width_125) =
            pixel_aligned_stroke(Rect::new(0.0, 0.0, 100.0, 40.0), 1.0, 1.25).unwrap();
        close(width_125 * 1.25, 1.0);
        close(path_125.left() * 1.25, 0.5);
        close(path_125.right() * 1.25, 124.5);

        let (path_150, width_150) =
            pixel_aligned_stroke(Rect::new(0.0, 0.0, 100.0, 40.0), 1.0, 1.5).unwrap();
        close(width_150 * 1.5, 2.0);
        close(path_150.left() * 1.5, 1.0);
        close(path_150.right() * 1.5, 149.0);

        let (_, width_200) =
            pixel_aligned_stroke(Rect::new(0.0, 0.0, 100.0, 40.0), 1.0, 2.0).unwrap();
        close(width_200 * 2.0, 2.0);
    }

    #[test]
    fn 对齐后的描边不会超出原矩形() {
        let rect = Rect::new(0.2, 0.2, 10.4, 8.4);
        let (path, width) = pixel_aligned_stroke(rect, 1.0, 1.25).unwrap();
        let half = width / 2.0;
        assert!(path.left() - half >= rect.left());
        assert!(path.top() - half >= rect.top());
        assert!(path.right() + half <= rect.right());
        assert!(path.bottom() + half <= rect.bottom());
    }

    #[test]
    fn 无效或过小的描边区域被拒绝() {
        assert!(pixel_aligned_stroke(Rect::new(0.0, 0.0, 0.5, 0.5), 1.0, 1.0).is_none());
        assert!(pixel_aligned_stroke(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, 1.0).is_none());
        assert!(pixel_aligned_stroke(Rect::new(0.0, 0.0, 10.0, 10.0), 1.0, 0.0).is_none());
    }

    #[test]
    fn 仿射变换可组合并求逆() {
        let transform = Affine::translation(-10.0, -5.0)
            .then(Affine::scale(2.0, 3.0))
            .then(Affine::rotation(std::f32::consts::FRAC_PI_2))
            .then(Affine::translation(10.0, 5.0));
        let point = Point::new(14.0, 7.0);
        let transformed = transform.transform_point(point);
        close(transformed.x, 4.0);
        close(transformed.y, 13.0);
        let restored = transform.inverse().unwrap().transform_point(transformed);
        close(restored.x, point.x);
        close(restored.y, point.y);
    }

    #[test]
    fn 仿射变换返回矩形包围盒() {
        let bounds = Affine::rotation(std::f32::consts::FRAC_PI_2)
            .transform_rect(Rect::new(0.0, 0.0, 20.0, 10.0));
        close(bounds.left(), -10.0);
        close(bounds.top(), 0.0);
        close(bounds.size.width, 10.0);
        close(bounds.size.height, 20.0);
    }
}
