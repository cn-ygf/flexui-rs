//! flexui-gfx：平台无关的几何与绘图抽象层（L2）。
//!
//! 定义几何、颜色、字体描述以及「画什么」的接口 `Canvas`，不含任何平台实现。
//! macOS / Windows 后端各自实现 `Canvas`，上层控件面向本接口自绘，彻底与平台解耦。

mod geometry;

pub use geometry::{pixel_aligned_stroke, Affine, Color, Corners, Insets, Point, Rect, Size};

use std::any::Any;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

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

/// 一行文字经过 shaping 后的字符边界。
///
/// `char_index` 使用 Unicode scalar value 索引，以保持现有控件代码 API 兼容；平台
/// 后端负责在 UTF-8、UTF-16 与平台文字引擎索引之间转换。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBoundary {
    pub char_index: usize,
    pub x: f32,
}

/// 平台文字引擎产生的一行排版结果。
///
/// `platform_data` 保存 CoreText/DirectWrite 的原生排版对象，确保绘制与光标、选区、
/// 命中测试消费同一次 shaping 的结果。
#[derive(Clone)]
pub struct TextLayout {
    text: String,
    font: Font,
    size: Size,
    ascent: f32,
    descent: f32,
    boundaries: Vec<TextBoundary>,
    platform_data: Option<Rc<dyn Any>>,
}

impl fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayout")
            .field("text", &self.text)
            .field("font", &self.font)
            .field("size", &self.size)
            .field("ascent", &self.ascent)
            .field("descent", &self.descent)
            .field("boundaries", &self.boundaries)
            .finish_non_exhaustive()
    }
}

impl TextLayout {
    pub fn new(
        text: impl Into<String>,
        font: Font,
        size: Size,
        ascent: f32,
        descent: f32,
        boundaries: Vec<TextBoundary>,
    ) -> Self {
        Self {
            text: text.into(),
            font,
            size,
            ascent,
            descent,
            boundaries,
            platform_data: None,
        }
    }

    /// 附加平台原生排版对象；仅对应平台 Canvas 会读取它。
    pub fn with_platform_data(mut self, data: Rc<dyn Any>) -> Self {
        self.platform_data = Some(data);
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn width(&self) -> f32 {
        self.size.width
    }

    pub fn height(&self) -> f32 {
        self.size.height
    }

    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    pub fn descent(&self) -> f32 {
        self.descent
    }

    pub fn char_count(&self) -> usize {
        self.boundaries.len().saturating_sub(1)
    }

    pub fn boundaries(&self) -> &[TextBoundary] {
        &self.boundaries
    }

    pub fn platform_data<T: Any>(&self) -> Option<&T> {
        self.platform_data.as_deref()?.downcast_ref::<T>()
    }

    /// 返回字符边界的视觉 x 坐标。
    pub fn x_for_char(&self, char_index: usize) -> f32 {
        self.boundaries
            .get(char_index.min(self.char_count()))
            .map_or(0.0, |boundary| boundary.x)
    }

    /// 返回离指定 x 最近的字符边界。按所有视觉边界比较，可兼容 RTL 与混排。
    pub fn closest_char_for_x(&self, x: f32) -> usize {
        self.boundaries
            .iter()
            .min_by(|a, b| {
                (a.x - x)
                    .abs()
                    .total_cmp(&(b.x - x).abs())
                    .then_with(|| a.char_index.cmp(&b.char_index))
            })
            .map_or(0, |boundary| boundary.char_index)
    }

    /// 把逻辑选区拆成视觉矩形。双向文字可能返回多个不连续矩形。
    pub fn selection_rects(&self, range: Range<usize>, y: f32, height: f32) -> Vec<Rect> {
        let start = range.start.min(self.char_count());
        let end = range.end.min(self.char_count());
        if start >= end {
            return Vec::new();
        }

        let mut spans = (start..end)
            .map(|index| {
                let a = self.x_for_char(index);
                let b = self.x_for_char(index + 1);
                (a.min(b), a.max(b))
            })
            .collect::<Vec<_>>();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut merged: Vec<(f32, f32)> = Vec::new();
        for (left, right) in spans {
            if let Some(last) = merged.last_mut().filter(|last| left <= last.1 + 0.5) {
                last.1 = last.1.max(right);
            } else {
                merged.push((left, right));
            }
        }
        merged
            .into_iter()
            .map(|(left, right)| Rect::new(left, y, (right - left).max(1.0), height))
            .collect()
    }
}

/// 离屏图层句柄：把一段内容预渲染成位图，之后可整块 blit（用于滚动等只平移的内容）。
///
/// `data` 用不透明的 `Rc<dyn Any>` 承载各后端的位图对象（如 macOS 的 NSImage）。
#[derive(Clone)]
pub struct LayerHandle {
    /// 逻辑尺寸（点）。
    pub size: Size,
    /// 渲染时的像素密度（HiDPI 匹配用）。
    pub scale: f32,
    data: Rc<dyn Any>,
}

impl LayerHandle {
    pub fn new(size: Size, scale: f32, data: Rc<dyn Any>) -> Self {
        Self { size, scale, data }
    }
    /// 取回后端位图对象。
    pub fn data<T: Any>(&self) -> Option<&T> {
        self.data.downcast_ref::<T>()
    }
}

impl fmt::Debug for LayerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayerHandle")
            .field("size", &self.size)
            .field("scale", &self.scale)
            .finish()
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

    /// 按文本前进边界绘制，供输入控件与插入光标共用同一套排版边界。
    fn draw_text_advance(&mut self, text: &str, origin: Point, font: &Font, color: Color) {
        self.draw_text(text, origin, font, color);
    }

    /// 度量一行文字的尺寸（用于布局）。
    fn measure_text(&self, text: &str, font: &Font) -> Size;

    /// 测量与 `draw_text_advance` 相同排版方式下的文字尺寸。
    fn measure_text_advance_size(&self, text: &str, font: &Font) -> Size {
        self.measure_text(text, font)
    }

    /// 测量文本排版前进宽度，供插入光标和选区使用。
    fn measure_text_advance(&self, text: &str, font: &Font) -> f32 {
        self.measure_text_advance_size(text, font).width
    }

    /// 使用平台成熟文字引擎对一行文字进行 shaping。
    ///
    /// 缺省实现只用于测试画布和第三方旧后端兼容；macOS/Windows 后端必须覆写。
    fn layout_text(&self, text: &str, font: &Font) -> TextLayout {
        let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
        for index in 0..=text.chars().count() {
            let prefix: String = text.chars().take(index).collect();
            boundaries.push(TextBoundary {
                char_index: index,
                x: self.measure_text_advance(&prefix, font),
            });
        }
        let size = self.measure_text_advance_size(text, font);
        TextLayout::new(
            text,
            font.clone(),
            size,
            font.size,
            (size.height - font.size).max(0.0),
            boundaries,
        )
    }

    /// 绘制已有排版结果。平台后端覆写后必须直接消费其中的原生排版对象。
    fn draw_text_layout(&mut self, layout: &TextLayout, origin: Point, color: Color) {
        self.draw_text_advance(layout.text(), origin, layout.font(), color);
    }

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

    /// 把仿射变换追加到当前绘图状态；通常与 `save` / `restore` 配合使用。
    fn concat_transform(&mut self, _transform: Affine) {}

    /// 追加矩形裁剪区（缺省为空，后端可覆写）。
    fn clip_rect(&mut self, _rect: Rect) {}

    /// 追加圆角矩形裁剪区；不支持路径裁剪的后端回退为矩形裁剪。
    fn clip_round_rect(&mut self, rect: Rect, _radius: Corners) {
        self.clip_rect(rect);
    }

    /// 当前画布的像素密度（离屏图层匹配用）。
    fn scale(&self) -> f32 {
        1.0
    }

    /// 在离屏位图里渲染一段 `size` 大小的内容（左上原点坐标系与主画布一致），返回可后续
    /// blit 的句柄。返回 `None` 表示该后端不支持离屏渲染，调用方应退化为直接绘制。
    fn capture_layer(
        &mut self,
        _size: Size,
        _draw: &mut dyn FnMut(&mut dyn Canvas),
    ) -> Option<LayerHandle> {
        None
    }

    /// 把离屏图层贴回当前画布，左上角在 `origin`（逻辑坐标）。缺省为空。
    fn draw_layer(&mut self, _layer: &LayerHandle, _origin: Point) {}
}

#[cfg(test)]
mod tests {
    use super::{image_density_from_path, Font, ImageSource, Size, TextBoundary, TextLayout};

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

    #[test]
    fn 文字布局按视觉边界命中并拆分选区() {
        let layout = TextLayout::new(
            "abc",
            Font::system(14.0),
            Size::new(30.0, 18.0),
            14.0,
            4.0,
            vec![
                TextBoundary {
                    char_index: 0,
                    x: 0.0,
                },
                TextBoundary {
                    char_index: 1,
                    x: 10.0,
                },
                TextBoundary {
                    char_index: 2,
                    x: 20.0,
                },
                TextBoundary {
                    char_index: 3,
                    x: 30.0,
                },
            ],
        );
        assert_eq!(layout.closest_char_for_x(16.0), 2);
        let rects = layout.selection_rects(1..3, 2.0, 18.0);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].size.width, 20.0);

        let rtl = TextLayout::new(
            "אב",
            Font::system(14.0),
            Size::new(20.0, 18.0),
            14.0,
            4.0,
            vec![
                TextBoundary {
                    char_index: 0,
                    x: 20.0,
                },
                TextBoundary {
                    char_index: 1,
                    x: 10.0,
                },
                TextBoundary {
                    char_index: 2,
                    x: 0.0,
                },
            ],
        );
        assert_eq!(rtl.closest_char_for_x(2.0), 2);
        assert_eq!(rtl.selection_rects(0..2, 0.0, 18.0).len(), 1);
    }
}
