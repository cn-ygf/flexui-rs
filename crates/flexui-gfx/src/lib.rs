//! flexui-gfx：平台无关的绘图抽象层（L2）。
//!
//! 只定义「画什么」的接口 `Canvas` 与字体描述 `Font`，不含任何平台实现。
//! macOS / Windows 后端各自实现 `Canvas`，上层控件面向本接口自绘，彻底与平台解耦。

use flexui_geometry::{Color, Corners, Insets, Point, Rect, Size};

/// 图片来源：磁盘文件路径 / 内存位图字节 / SVG 字节。
#[derive(Debug, Clone, PartialEq)]
pub enum ImageSource {
    /// 磁盘文件路径（位图，OS 解码）。
    Path(String),
    /// 内存位图字节（PNG/JPG/... 经资源解析，OS 解码）。
    Bytes(std::sync::Arc<Vec<u8>>),
    /// 带像素密度的磁盘位图；density=2 表示 2 个物理像素对应 1 个逻辑像素。
    ScaledPath(String, f32),
    /// 带像素密度的内存位图。
    ScaledBytes(std::sync::Arc<Vec<u8>>, f32),
    /// SVG 字节（按目标尺寸光栅化）。
    Svg(std::sync::Arc<Vec<u8>>),
}

impl ImageSource {
    pub fn path(p: impl Into<String>) -> Self {
        let path = p.into();
        let density = image_density_from_path(&path);
        if density == 1.0 {
            ImageSource::Path(path)
        } else {
            ImageSource::ScaledPath(path, density)
        }
    }
    pub fn bytes(b: impl Into<Vec<u8>>) -> Self {
        ImageSource::Bytes(std::sync::Arc::new(b.into()))
    }
    pub fn svg(b: impl Into<Vec<u8>>) -> Self {
        ImageSource::Svg(std::sync::Arc::new(b.into()))
    }
    pub fn path_scaled(p: impl Into<String>, density: f32) -> Self {
        ImageSource::ScaledPath(p.into(), valid_density(density))
    }
    pub fn bytes_scaled(b: impl Into<Vec<u8>>, density: f32) -> Self {
        ImageSource::ScaledBytes(std::sync::Arc::new(b.into()), valid_density(density))
    }
    pub fn density(&self) -> f32 {
        match self {
            ImageSource::ScaledPath(_, density) | ImageSource::ScaledBytes(_, density) => {
                valid_density(*density)
            }
            _ => 1.0,
        }
    }
    pub fn is_svg(&self) -> bool {
        matches!(self, ImageSource::Svg(_))
    }
}

fn valid_density(density: f32) -> f32 {
    if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    }
}

/// 从 `icon@2.00x.png` 一类文件名解析像素密度；无倍率后缀时返回 1。
pub fn image_density_from_path(path: &str) -> f32 {
    let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
    let Some((_, suffix)) = stem.rsplit_once('@') else {
        return 1.0;
    };
    let Some(number) = suffix
        .strip_suffix('x')
        .or_else(|| suffix.strip_suffix('X'))
    else {
        return 1.0;
    };
    number.parse::<f32>().map_or(1.0, valid_density)
}

/// 图片渲染方式（I4）。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ImageFit {
    /// 整体拉伸填充目标矩形（默认）。
    #[default]
    Stretch,
    /// 原始尺寸居中。
    Center,
    /// 平铺。
    Tile,
    /// 九宫格：四边不拉伸、中间拉伸（Insets 为源图四边不拉伸的边距）。
    NinePatch(Insets),
}

/// 文字水平对齐（用于按钮/标签内容）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 字体描述：字号、字族名与样式（粗体/斜体/下划线）。
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    /// 字族名，None 表示用系统默认字体。
    pub family: Option<String>,
    /// 字号（逻辑像素/point）。
    pub size: f32,
    /// 粗体。
    pub bold: bool,
    /// 斜体。
    pub italic: bool,
    /// 下划线。
    pub underline: bool,
}

impl Font {
    pub fn system(size: f32) -> Self {
        Self {
            family: None,
            size,
            bold: false,
            italic: false,
            underline: false,
        }
    }
    /// 指定字族名。
    pub fn family(mut self, name: impl Into<String>) -> Self {
        self.family = Some(name.into());
        self
    }
    /// 设置粗体。
    pub fn bold(mut self, on: bool) -> Self {
        self.bold = on;
        self
    }
    /// 设置斜体。
    pub fn italic(mut self, on: bool) -> Self {
        self.italic = on;
        self
    }
    /// 设置下划线。
    pub fn underline(mut self, on: bool) -> Self {
        self.underline = on;
        self
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

    /// 用两色线性渐变填充（圆角）矩形；`vertical`=true 上→下，false 左→右。
    /// 缺省实现回退为起始色纯填充；后端覆写为真渐变。
    fn fill_gradient_rect(
        &mut self,
        rect: Rect,
        radius: Corners,
        from: Color,
        _to: Color,
        _vertical: bool,
    ) {
        self.fill_round_rect(rect, radius, from);
    }

    /// 描边圆角矩形（用于边框）。
    fn stroke_round_rect(&mut self, rect: Rect, radius: Corners, color: Color, line_width: f32);

    /// 在指定位置绘制一行文字（左上角对齐 origin）。
    fn draw_text(&mut self, text: &str, origin: Point, font: &Font, color: Color);

    /// 度量一行文字的尺寸（用于布局）。
    fn measure_text(&self, text: &str, font: &Font) -> Size;

    /// 在矩形内绘制图片，支持换色 tint 与渲染方式 fit（缺省为空，后端覆写）。
    /// tint 为 Some 时按目标色重着色（保留 alpha 形状），用于「黑图动态换色」。
    fn draw_image(
        &mut self,
        _source: &ImageSource,
        _rect: Rect,
        _tint: Option<Color>,
        _fit: ImageFit,
    ) {
    }

    /// 保存绘图状态（配合 clip 使用）。
    fn save(&mut self) {}

    /// 恢复绘图状态。
    fn restore(&mut self) {}

    /// 追加矩形裁剪区（缺省为空，后端可覆写）。
    fn clip_rect(&mut self, _rect: Rect) {}

    /// 追加圆角矩形裁剪区；不支持路径裁剪的后端回退为矩形裁剪。
    fn clip_round_rect(&mut self, rect: Rect, _radius: Corners) {
        self.clip_rect(rect);
    }
}

#[cfg(test)]
mod tests {
    use super::{image_density_from_path, Font, ImageSource};

    #[test]
    fn font_样式构建() {
        let f = Font::system(16.0)
            .bold(true)
            .italic(true)
            .underline(true)
            .family("Menlo");
        assert_eq!(f.size, 16.0);
        assert!(f.bold && f.italic && f.underline);
        assert_eq!(f.family.as_deref(), Some("Menlo"));
        // 默认无样式
        let d = Font::default();
        assert!(!d.bold && !d.italic && !d.underline);
    }

    #[test]
    fn 图片文件名_解析像素密度() {
        assert_eq!(image_density_from_path("button/close@2.00x.png"), 2.0);
        assert_eq!(image_density_from_path("icon@1.25x.PNG"), 1.25);
        assert_eq!(image_density_from_path("icon.png"), 1.0);
        assert_eq!(image_density_from_path("icon@badx.png"), 1.0);
        assert_eq!(ImageSource::path("icon@2.00x.png").density(), 2.0);
    }
}
