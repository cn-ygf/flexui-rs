//! 控件视觉变换与命中形状。

use flexui_gfx::{Affine, Corners, Point, Rect};

/// 控件及其整棵子树的视觉变换；布局矩形本身保持不变。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetTransform {
    /// 变换完成后的额外平移（逻辑像素）。
    pub translation: Point,
    pub scale_x: f32,
    pub scale_y: f32,
    /// 顺时针旋转角度（当前坐标系 y 向下）。
    pub rotation_degrees: f32,
    /// 变换中心占布局矩形宽高的比例，默认 `(0.5, 0.5)`。
    pub origin: Point,
}

impl WidgetTransform {
    pub const IDENTITY: Self = Self {
        translation: Point::new(0.0, 0.0),
        scale_x: 1.0,
        scale_y: 1.0,
        rotation_degrees: 0.0,
        origin: Point::new(0.5, 0.5),
    };

    pub fn affine(self, rect: Rect) -> Affine {
        let finite = |value: f32, fallback: f32| {
            if value.is_finite() {
                value
            } else {
                fallback
            }
        };
        let origin = Point::new(
            rect.left() + rect.size.width * finite(self.origin.x, 0.5),
            rect.top() + rect.size.height * finite(self.origin.y, 0.5),
        );
        let translation = Point::new(
            finite(self.translation.x, 0.0),
            finite(self.translation.y, 0.0),
        );
        let scale_x = finite(self.scale_x, 1.0);
        let scale_y = finite(self.scale_y, 1.0);
        let radians = finite(self.rotation_degrees, 0.0).to_radians();
        Affine::translation(-origin.x, -origin.y)
            .then(Affine::scale(scale_x, scale_y))
            .then(Affine::rotation(radians))
            .then(Affine::translation(origin.x, origin.y))
            .then(Affine::translation(translation.x, translation.y))
    }

    pub fn is_identity(self) -> bool {
        self.translation == Point::new(0.0, 0.0)
            && self.scale_x == 1.0
            && self.scale_y == 1.0
            && self.rotation_degrees == 0.0
    }
}

impl Default for WidgetTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// 控件自身接收指针事件的形状。子控件仍按各自形状独立命中。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum HitShape {
    #[default]
    Rect,
    Rounded(Corners),
    Ellipse,
}

impl HitShape {
    pub fn contains(self, rect: Rect, point: Point) -> bool {
        if !rect.contains(point) {
            return false;
        }
        match self {
            Self::Rect => true,
            Self::Ellipse => {
                let rx = rect.size.width / 2.0;
                let ry = rect.size.height / 2.0;
                if rx <= 0.0 || ry <= 0.0 {
                    return false;
                }
                let nx = (point.x - (rect.left() + rx)) / rx;
                let ny = (point.y - (rect.top() + ry)) / ry;
                nx * nx + ny * ny <= 1.0
            }
            Self::Rounded(corners) => rounded_contains(rect, point, corners),
        }
    }
}

fn rounded_contains(rect: Rect, point: Point, corners: Corners) -> bool {
    let max_radius = rect.size.width.min(rect.size.height).max(0.0) / 2.0;
    let checks = [
        (
            corners.tl.clamp(0.0, max_radius),
            rect.left(),
            rect.top(),
            1.0,
            1.0,
        ),
        (
            corners.tr.clamp(0.0, max_radius),
            rect.right(),
            rect.top(),
            -1.0,
            1.0,
        ),
        (
            corners.br.clamp(0.0, max_radius),
            rect.right(),
            rect.bottom(),
            -1.0,
            -1.0,
        ),
        (
            corners.bl.clamp(0.0, max_radius),
            rect.left(),
            rect.bottom(),
            1.0,
            -1.0,
        ),
    ];
    for (radius, corner_x, corner_y, x_dir, y_dir) in checks {
        if radius <= 0.0 {
            continue;
        }
        let center_x = corner_x + x_dir * radius;
        let center_y = corner_y + y_dir * radius;
        let in_corner_x = (point.x - corner_x) * x_dir < radius;
        let in_corner_y = (point.y - corner_y) * y_dir < radius;
        if in_corner_x && in_corner_y {
            let dx = point.x - center_x;
            let dy = point.y - center_y;
            if dx * dx + dy * dy > radius * radius {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }

    #[test]
    fn 控件变换围绕指定中心组合() {
        let transform = WidgetTransform {
            translation: Point::new(4.0, -2.0),
            scale_x: 2.0,
            scale_y: 1.0,
            rotation_degrees: 90.0,
            origin: Point::new(0.5, 0.5),
        }
        .affine(Rect::new(10.0, 20.0, 20.0, 10.0));
        let point = transform.transform_point(Point::new(30.0, 25.0));
        close(point.x, 24.0);
        close(point.y, 43.0);
    }

    #[test]
    fn 圆角与椭圆命中排除透明角落() {
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert!(!HitShape::Rounded(Corners::all(12.0)).contains(rect, Point::new(1.0, 1.0)));
        assert!(HitShape::Rounded(Corners::all(12.0)).contains(rect, Point::new(12.0, 1.0)));
        assert!(!HitShape::Ellipse.contains(rect, Point::new(1.0, 1.0)));
        assert!(HitShape::Ellipse.contains(rect, Point::new(50.0, 20.0)));
    }
}
