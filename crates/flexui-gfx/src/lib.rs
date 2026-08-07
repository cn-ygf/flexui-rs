//! flexui-gfx：平台无关的绘图抽象层（L2）。
//!
//! 只定义「画什么」的接口 `Canvas` 与字体描述 `Font`，不含任何平台实现。
//! macOS / Windows 后端各自实现 `Canvas`，上层控件面向本接口自绘，彻底与平台解耦。

use flexui_geometry::{Color, Corners, Point, Rect, Size};

/// 图片来源：一期支持文件路径；后续可扩展内存字节/资源 id。
#[derive(Debug, Clone, PartialEq)]
pub enum ImageSource {
    /// 磁盘文件路径。
    Path(String),
}

impl ImageSource {
    pub fn path(p: impl Into<String>) -> Self {
        ImageSource::Path(p.into())
    }
}

/// 文字水平对齐（用于按钮/标签内容）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 字体描述（一期只含字号与字族名，后续再扩粗细/斜体等）。
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    /// 字族名，None 表示用系统默认字体。
    pub family: Option<String>,
    /// 字号（逻辑像素/point）。
    pub size: f32,
}

impl Font {
    pub fn system(size: f32) -> Self {
        Self {
            family: None,
            size,
        }
    }
}

impl Default for Font {
    fn default() -> Self {
        Font::system(14.0)
    }
}

/// 画布抽象：控件自绘时唯一面向的接口。
///
/// 坐标一律使用「逻辑像素、左上原点、y 向下」；缩放(HiDPI)由后端处理。
pub trait Canvas {
    /// 填充矩形。
    fn fill_rect(&mut self, rect: Rect, color: Color);

    /// 描边矩形。
    fn stroke_rect(&mut self, rect: Rect, color: Color, line_width: f32);

    /// 填充圆角矩形。
    fn fill_round_rect(&mut self, rect: Rect, radius: Corners, color: Color);

    /// 描边圆角矩形（用于边框）。
    fn stroke_round_rect(&mut self, rect: Rect, radius: Corners, color: Color, line_width: f32);

    /// 在指定位置绘制一行文字（左上角对齐 origin）。
    fn draw_text(&mut self, text: &str, origin: Point, font: &Font, color: Color);

    /// 度量一行文字的尺寸（用于布局）。
    fn measure_text(&self, text: &str, font: &Font) -> Size;

    /// 在矩形内绘制图片（一期整体缩放填充；缺省实现为空，后端可覆写）。
    fn draw_image(&mut self, _source: &ImageSource, _rect: Rect) {}

    /// 保存绘图状态（配合 clip 使用）。
    fn save(&mut self) {}

    /// 恢复绘图状态。
    fn restore(&mut self) {}

    /// 追加矩形裁剪区（缺省为空，后端可覆写）。
    fn clip_rect(&mut self, _rect: Rect) {}
}
