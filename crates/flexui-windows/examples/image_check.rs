//! Windows 图片绘制验证（wine 无头可跑）：运行时生成一张纯绿 BMP，用 Image 控件绘制到
//! 离屏位图，再读回中心像素断言为绿色 —— 验证 GdiCanvas::draw_image（GDI+ 图片加载/绘制）。

#[cfg(windows)]
fn main() {
    use flexui_core::{Color, Image, ImageSource, Panel, StyleSet, StyleSpec, Widget};

    // —— 生成一张 8x8 纯绿 24 位 BMP 到临时文件 ——
    let path = std::env::temp_dir().join("flexui_green.bmp");
    write_solid_bmp(&path, 8, 8, [0, 255, 0]).expect("写 BMP 失败");
    let path_str = path.to_string_lossy().to_string();
    println!("bmp = {path_str}");

    // 1) 原色绘制：期望绿色。
    let mut root = Panel::new().push(Image::new(ImageSource::path(path_str.clone())));
    let pixel = flexui_windows::render_tree_argb(&mut root as &mut dyn Widget, 40, 40, (20, 20))
        .expect("离屏渲染失败");
    println!("image pixel  = {pixel:#010X} (期望绿 0xFF00FF00)");
    let (r, g, b) = ((pixel >> 16) & 0xFF, (pixel >> 8) & 0xFF, pixel & 0xFF);
    let plain_ok = g > 180 && r < 80 && b < 80;

    // 2) 换色 tint：把绿图 tint 成红，期望红色（验证「黑图/任意图动态换色」）。
    let red = Color::from_u8(255, 0, 0, 255);
    let tint_style = StyleSet::new().with_normal(StyleSpec {
        fg_tint: Some(red),
        ..Default::default()
    });
    let mut root2 = Panel::new().push(Image::new(ImageSource::path(path_str)).style(tint_style));
    let px2 = flexui_windows::render_tree_argb(&mut root2 as &mut dyn Widget, 40, 40, (20, 20))
        .expect("离屏渲染失败");
    println!("tinted pixel = {px2:#010X} (期望红 0xFFFF0000)");
    let (tr, tg, tb) = ((px2 >> 16) & 0xFF, (px2 >> 8) & 0xFF, px2 & 0xFF);
    let tint_ok = tr > 180 && tg < 80 && tb < 80;

    let _ = std::fs::remove_file(&path);

    // 3) SVG：蓝色矩形 SVG，光栅化绘制，期望蓝色。
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#0000FF"/></svg>"##;
    let mut root3 = Panel::new().push(Image::new(ImageSource::svg(svg.to_vec())));
    let px3 = flexui_windows::render_tree_argb(&mut root3 as &mut dyn Widget, 40, 40, (20, 20))
        .expect("离屏渲染失败");
    println!("svg pixel    = {px3:#010X} (期望蓝 0xFF0000FF)");
    let (sr, sg, sb) = ((px3 >> 16) & 0xFF, (px3 >> 8) & 0xFF, px3 & 0xFF);
    let svg_ok = sb > 180 && sr < 80 && sg < 80;

    if plain_ok && tint_ok && svg_ok {
        println!("WIN-IMAGE-OK");
    } else {
        println!("WIN-IMAGE-FAIL (plain={plain_ok} tint={tint_ok} svg={svg_ok})");
        std::process::exit(1);
    }
}

/// 写一张 width×height 的 24 位未压缩 BMP（纯色 BGR）。
#[cfg(windows)]
fn write_solid_bmp(path: &std::path::Path, w: i32, h: i32, rgb: [u8; 3]) -> std::io::Result<()> {
    use std::io::Write;
    let row_bytes = ((w * 3 + 3) / 4) * 4; // 4 字节对齐
    let pixel_data = (row_bytes * h) as u32;
    let file_size = 54 + pixel_data;

    let mut buf: Vec<u8> = Vec::with_capacity(file_size as usize);
    // BITMAPFILEHEADER
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // reserved
    buf.extend_from_slice(&54u32.to_le_bytes()); // 像素数据偏移
    // BITMAPINFOHEADER
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&w.to_le_bytes());
    buf.extend_from_slice(&h.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // planes
    buf.extend_from_slice(&24u16.to_le_bytes()); // bpp
    buf.extend_from_slice(&0u32.to_le_bytes()); // 无压缩
    buf.extend_from_slice(&pixel_data.to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes()); // x ppm (~72dpi)
    buf.extend_from_slice(&2835u32.to_le_bytes()); // y ppm
    buf.extend_from_slice(&0u32.to_le_bytes()); // colors
    buf.extend_from_slice(&0u32.to_le_bytes()); // important
                                                 // 像素（BGR，行末补零对齐）
    let [red, green, blue] = rgb;
    for _ in 0..h {
        for _ in 0..w {
            buf.push(blue);
            buf.push(green);
            buf.push(red);
        }
        buf.resize(buf.len() + (row_bytes - w * 3) as usize, 0);
    }

    let mut f = std::fs::File::create(path)?;
    f.write_all(&buf)?;
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    println!("image_check 示例仅适用于 Windows 目标");
}
