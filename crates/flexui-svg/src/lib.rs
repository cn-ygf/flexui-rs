//! flexui-svg：SVG 光栅化（resvg，纯 Rust）。作用域仅「SVG 字节 → RGBA 位图」，
//! UI 主渲染仍是各平台原生。返回 RGBA(premultiplied) 字节，交后端建位图绘制。

use resvg::tiny_skia;
use resvg::usvg;

/// 读取 SVG 的固有逻辑尺寸（width/height 或 viewBox 解析后的尺寸）。
pub fn intrinsic_size(svg: &[u8]) -> Option<(f32, f32)> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        None
    } else {
        Some((size.width(), size.height()))
    }
}

/// 把 SVG 光栅化成 w×h 的 RGBA(premultiplied) 字节（每像素 4 字节，行优先）。失败返回 None。
pub fn rasterize(svg: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg, &opt).ok()?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if sw <= 0.0 || sh <= 0.0 {
        return None;
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let transform = tiny_skia::Transform::from_scale(w as f32 / sw, h as f32 / sh);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 光栅化红色矩形() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>"#;
        let rgba = rasterize(svg, 8, 8).expect("光栅化失败");
        assert_eq!(rgba.len(), 8 * 8 * 4);
        // 中心像素应为红色（R 高、G/B 低）。
        let i = (4 * 8 + 4) * 4;
        assert!(rgba[i] > 200, "R={}", rgba[i]);
        assert!(rgba[i + 1] < 60);
        assert!(rgba[i + 2] < 60);
        assert_eq!(intrinsic_size(svg), Some((10.0, 10.0)));
    }
}
