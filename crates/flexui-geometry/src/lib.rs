//! flexui-geometry：平台无关的几何与颜色基础类型（L2）。
//!
//! 统一使用「逻辑像素、左上原点、y 向下」的坐标系；平台后端负责把这里的类型
//! 转换成各自的原生类型（如 macOS 的 NSRect），上层控件只依赖本 crate。

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
        Rect::new(self.origin.x + insets.left, self.origin.y + insets.top, w, h)
    }

    /// 四边收缩相同值。
    pub fn inset_all(&self, v: f32) -> Rect {
        self.deflate(Insets::all(v))
    }
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
