//! DirectWrite 单行排版与绘制。

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::OnceLock;

use flexui_gfx::{Color, Point, Rect, Size};
use flexui_gfx::{Font, TextBoundary, TextLayout};
use windows::core::{implement, Ref, Result as WinResult, BOOL, HSTRING};
use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL, RECT};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_TEXTURE_CLEARTYPE_3x1, DWriteCreateFactory, IDWriteFactory, IDWriteInlineObject,
    IDWritePixelSnapping_Impl, IDWriteTextLayout, IDWriteTextRenderer, IDWriteTextRenderer_Impl,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_GLYPH_RUN,
    DWRITE_GLYPH_RUN_DESCRIPTION, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS, DWRITE_MATRIX,
    DWRITE_MEASURING_MODE, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC, DWRITE_STRIKETHROUGH,
    DWRITE_TEXT_METRICS, DWRITE_UNDERLINE, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows_sys::Win32::Graphics::GdiPlus as gp;

use crate::gdiplus::PIXEL_FORMAT_32BPP_PARGB;

const DEFAULT_FONT_FAMILY: &str = "Microsoft YaHei";

#[derive(Clone)]
pub(crate) struct DirectWriteLayout {
    layout: IDWriteTextLayout,
}

struct DirectWriteSystem {
    factory: IDWriteFactory,
    renderer: IDWriteTextRenderer,
}

impl DirectWriteSystem {
    fn new() -> WinResult<Self> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let renderer: IDWriteTextRenderer = AlphaMaskTextRenderer.into();
        Ok(Self { factory, renderer })
    }

    fn layout(&self, text: &str, font: &Font) -> WinResult<TextLayout> {
        let family = HSTRING::from(font.family.as_deref().unwrap_or(DEFAULT_FONT_FAMILY));
        let locale = HSTRING::from("zh-CN");
        let weight = if font.bold {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        let style = if font.italic {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        };
        let format = unsafe {
            self.factory.CreateTextFormat(
                &family,
                None,
                weight,
                style,
                DWRITE_FONT_STRETCH_NORMAL,
                font.size,
                &locale,
            )?
        };
        unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)? };
        let utf16 = text.encode_utf16().collect::<Vec<_>>();
        let layout = unsafe {
            self.factory
                .CreateTextLayout(&utf16, &format, 1_000_000.0, 1_000_000.0)?
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        let mut line_metrics = [DWRITE_LINE_METRICS::default(); 1];
        let mut line_count = 0;
        unsafe { layout.GetLineMetrics(Some(&mut line_metrics), &mut line_count)? };
        let (ascent, descent, height) = if line_count > 0 {
            let line = line_metrics[0];
            (line.baseline, line.height - line.baseline, line.height)
        } else {
            (font.size, font.size * 0.2, font.size * 1.2)
        };

        let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
        let mut utf16_index = 0u32;
        boundaries.push(TextBoundary {
            char_index: 0,
            x: hit_test_x(&layout, 0)?,
        });
        for (char_index, ch) in text.chars().enumerate() {
            utf16_index += ch.len_utf16() as u32;
            boundaries.push(TextBoundary {
                char_index: char_index + 1,
                x: hit_test_x(&layout, utf16_index)?,
            });
        }
        let width = metrics.widthIncludingTrailingWhitespace.max(metrics.width);
        let size = Size::new(width, height.max(ascent + descent));
        Ok(
            TextLayout::new(text, font.clone(), size, ascent, descent, boundaries)
                .with_platform_data(Rc::new(DirectWriteLayout { layout })),
        )
    }

    unsafe fn draw(
        &self,
        graphics: *mut gp::GpGraphics,
        scale: f32,
        clip: Option<Rect>,
        layout: &DirectWriteLayout,
        origin: Point,
        color: Color,
    ) -> WinResult<()> {
        if color.a <= 0.0 {
            return Ok(());
        }
        let scale = scale.max(0.1);
        let context = DrawContext {
            factory: &self.factory,
            scale,
            clip: clip.map(|rect| physical_rect(rect, scale)),
            color: color_bytes(color),
            glyphs: RefCell::new(Vec::new()),
        };
        unsafe {
            layout.layout.Draw(
                Some((&context as *const DrawContext).cast::<c_void>()),
                &self.renderer,
                origin.x,
                origin.y,
            )?;
        }

        for glyph in context.glyphs.into_inner() {
            let width = glyph.bounds.right - glyph.bounds.left;
            let height = glyph.bounds.bottom - glyph.bounds.top;
            let mut bitmap: *mut gp::GpBitmap = std::ptr::null_mut();
            if gp::GdipCreateBitmapFromScan0(
                width,
                height,
                width * 4,
                PIXEL_FORMAT_32BPP_PARGB,
                glyph.pixels.as_ptr(),
                &mut bitmap,
            ) != 0
                || bitmap.is_null()
            {
                return Err(E_FAIL.into());
            }
            let status = gp::GdipDrawImageRectRect(
                graphics,
                bitmap as *mut gp::GpImage,
                glyph.bounds.left as f32 / scale,
                glyph.bounds.top as f32 / scale,
                width as f32 / scale,
                height as f32 / scale,
                0.0,
                0.0,
                width as f32,
                height as f32,
                gp::UnitPixel,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            );
            gp::GdipDisposeImage(bitmap as *mut gp::GpImage);
            if status != 0 {
                return Err(E_FAIL.into());
            }
        }
        Ok(())
    }
}

fn hit_test_x(layout: &IDWriteTextLayout, utf16_index: u32) -> WinResult<f32> {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut metrics = DWRITE_HIT_TEST_METRICS::default();
    unsafe { layout.HitTestTextPosition(utf16_index, false, &mut x, &mut y, &mut metrics)? };
    Ok(x)
}

fn system() -> Option<&'static DirectWriteSystem> {
    static SYSTEM: OnceLock<Option<DirectWriteSystem>> = OnceLock::new();
    SYSTEM
        .get_or_init(|| DirectWriteSystem::new().ok())
        .as_ref()
}

pub(crate) fn layout_text(text: &str, font: &Font) -> Option<TextLayout> {
    system()?.layout(text, font).ok()
}

pub(crate) unsafe fn draw_text_layout(
    graphics: *mut gp::GpGraphics,
    scale: f32,
    clip: Option<Rect>,
    layout: &TextLayout,
    origin: Point,
    color: Color,
) -> bool {
    let Some(native) = layout.platform_data::<DirectWriteLayout>() else {
        return false;
    };
    system().is_some_and(|system| unsafe {
        system
            .draw(graphics, scale, clip, native, origin, color)
            .is_ok()
    })
}

fn physical_rect(rect: Rect, scale: f32) -> RECT {
    RECT {
        left: (rect.left() * scale).floor() as i32,
        top: (rect.top() * scale).floor() as i32,
        right: (rect.right() * scale).ceil() as i32,
        bottom: (rect.bottom() * scale).ceil() as i32,
    }
}

fn intersect_rect(a: RECT, b: RECT) -> RECT {
    let left = a.left.max(b.left);
    let top = a.top.max(b.top);
    RECT {
        left,
        top,
        right: a.right.min(b.right).max(left),
        bottom: a.bottom.min(b.bottom).max(top),
    }
}

fn color_bytes(color: Color) -> [u8; 4] {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(color.r), byte(color.g), byte(color.b), byte(color.a)]
}

fn alpha_texture_to_pargb(texture: &[u8], color: [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(texture.len() / 3 * 4);
    for coverage in texture.chunks_exact(3) {
        // GDI+ 只支持单 alpha，取 ClearType 三通道均值生成灰度抗锯齿蒙版。
        let coverage =
            (u16::from(coverage[0]) + u16::from(coverage[1]) + u16::from(coverage[2]) + 1) / 3;
        let alpha = (coverage * u16::from(color[3]) + 127) / 255;
        let premultiply = |channel: u8| ((u16::from(channel) * alpha + 127) / 255) as u8;
        pixels.extend_from_slice(&[
            premultiply(color[2]),
            premultiply(color[1]),
            premultiply(color[0]),
            alpha as u8,
        ]);
    }
    pixels
}

struct GlyphBitmap {
    bounds: RECT,
    pixels: Vec<u8>,
}

struct DrawContext<'a> {
    factory: &'a IDWriteFactory,
    scale: f32,
    clip: Option<RECT>,
    color: [u8; 4],
    glyphs: RefCell<Vec<GlyphBitmap>>,
}

#[implement(IDWriteTextRenderer)]
struct AlphaMaskTextRenderer;

impl IDWritePixelSnapping_Impl for AlphaMaskTextRenderer_Impl {
    fn IsPixelSnappingDisabled(&self, _context: *const c_void) -> WinResult<BOOL> {
        Ok(false.into())
    }

    fn GetCurrentTransform(
        &self,
        _context: *const c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> WinResult<()> {
        if !transform.is_null() {
            unsafe {
                *transform = DWRITE_MATRIX {
                    m11: 1.0,
                    m12: 0.0,
                    m21: 0.0,
                    m22: 1.0,
                    dx: 0.0,
                    dy: 0.0,
                };
            }
        }
        Ok(())
    }

    fn GetPixelsPerDip(&self, context: *const c_void) -> WinResult<f32> {
        if context.is_null() {
            Ok(1.0)
        } else {
            Ok(unsafe { (*(context.cast::<DrawContext>())).scale })
        }
    }
}

impl IDWriteTextRenderer_Impl for AlphaMaskTextRenderer_Impl {
    fn DrawGlyphRun(
        &self,
        context: *const c_void,
        baseline_x: f32,
        baseline_y: f32,
        measuring_mode: DWRITE_MEASURING_MODE,
        glyph_run: *const DWRITE_GLYPH_RUN,
        _description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _effect: Ref<'_, windows::core::IUnknown>,
    ) -> WinResult<()> {
        if context.is_null() || glyph_run.is_null() {
            return Ok(());
        }
        let context = unsafe { &*(context.cast::<DrawContext>()) };
        let analysis = unsafe {
            context.factory.CreateGlyphRunAnalysis(
                glyph_run,
                context.scale,
                None,
                DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                measuring_mode,
                baseline_x,
                baseline_y,
            )?
        };
        let mut bounds = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1)? };
        if let Some(clip) = context.clip {
            bounds = intersect_rect(bounds, clip);
        }
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        if width <= 0 || height <= 0 {
            return Ok(());
        }
        let mut texture = vec![0; width as usize * height as usize * 3];
        unsafe {
            analysis.CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, &mut texture)?;
        }
        context.glyphs.borrow_mut().push(GlyphBitmap {
            bounds,
            pixels: alpha_texture_to_pargb(&texture, context.color),
        });
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _context: *const c_void,
        _baseline_x: f32,
        _baseline_y: f32,
        _underline: *const DWRITE_UNDERLINE,
        _effect: Ref<'_, windows::core::IUnknown>,
    ) -> WinResult<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _context: *const c_void,
        _baseline_x: f32,
        _baseline_y: f32,
        _strikethrough: *const DWRITE_STRIKETHROUGH,
        _effect: Ref<'_, windows::core::IUnknown>,
    ) -> WinResult<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        _context: *const c_void,
        _origin_x: f32,
        _origin_y: f32,
        _inline_object: Ref<'_, IDWriteInlineObject>,
        _sideways: BOOL,
        _rtl: BOOL,
        _effect: Ref<'_, windows::core::IUnknown>,
    ) -> WinResult<()> {
        Err(E_NOTIMPL.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 逻辑裁剪按dpi转换为物理像素() {
        let rect = physical_rect(Rect::new(10.25, 4.25, 20.0, 10.0), 2.0);
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (20, 8, 61, 29)
        );
    }

    #[test]
    fn 物理裁剪无交集时返回空区域() {
        let rect = intersect_rect(
            RECT {
                left: 30,
                top: 30,
                right: 40,
                bottom: 40,
            },
            RECT {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            },
        );
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (30, 30, 30, 30)
        );
    }

    #[test]
    fn 文字透明度转换覆盖边界值() {
        assert_eq!(color_bytes(Color::rgba(0.0, 0.0, 0.0, -1.0))[3], 0);
        assert_eq!(color_bytes(Color::rgba(0.0, 0.0, 0.0, 0.5))[3], 128);
        assert_eq!(color_bytes(Color::rgba(0.0, 0.0, 0.0, 2.0))[3], 255);
    }

    #[test]
    fn 字形覆盖率转换为预乘argb() {
        assert_eq!(
            alpha_texture_to_pargb(&[255, 255, 255], [128, 64, 32, 128]),
            [16, 32, 64, 128]
        );
        assert_eq!(
            alpha_texture_to_pargb(&[0, 0, 0], [128, 64, 32, 128]),
            [0, 0, 0, 0]
        );
    }
}
