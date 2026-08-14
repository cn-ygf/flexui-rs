//! DirectWrite 单行排版与绘制。

use std::ffi::c_void;
use std::rc::Rc;
use std::sync::OnceLock;

use flexui_geometry::{Color, Point, Rect, Size};
use flexui_gfx::{Font, TextBoundary, TextLayout};
use windows::core::{implement, Ref, Result as WinResult, BOOL, HSTRING};
use windows::Win32::Foundation::{COLORREF, E_FAIL, E_NOTIMPL};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteBitmapRenderTarget, IDWriteFactory, IDWriteGdiInterop,
    IDWriteInlineObject, IDWritePixelSnapping_Impl, IDWriteRenderingParams, IDWriteTextLayout,
    IDWriteTextRenderer, IDWriteTextRenderer_Impl, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_GLYPH_RUN,
    DWRITE_GLYPH_RUN_DESCRIPTION, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS, DWRITE_MATRIX,
    DWRITE_MEASURING_MODE, DWRITE_STRIKETHROUGH, DWRITE_TEXT_METRICS, DWRITE_UNDERLINE,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Gdi::{BitBlt, GetCurrentObject, HDC, OBJ_BITMAP, SRCCOPY};
use windows_sys::Win32::Graphics::GdiPlus as gp;

const DEFAULT_FONT_FAMILY: &str = "Microsoft YaHei";

#[derive(Clone)]
pub(crate) struct DirectWriteLayout {
    layout: IDWriteTextLayout,
    size: Size,
}

struct RenderedTextBitmap {
    _target: IDWriteBitmapRenderTarget,
    bitmap: *mut gp::GpBitmap,
    scale: f32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Drop for RenderedTextBitmap {
    fn drop(&mut self) {
        if !self.bitmap.is_null() {
            unsafe { gp::GdipDisposeImage(self.bitmap as *mut gp::GpImage) };
        }
    }
}

struct DirectWriteSystem {
    factory: IDWriteFactory,
    gdi: IDWriteGdiInterop,
    renderer: IDWriteTextRenderer,
}

impl DirectWriteSystem {
    fn new() -> WinResult<Self> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let gdi = unsafe { factory.GetGdiInterop()? };
        let renderer: IDWriteTextRenderer = BitmapTextRenderer.into();
        Ok(Self {
            factory,
            gdi,
            renderer,
        })
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
                .with_platform_data(Rc::new(DirectWriteLayout { layout, size })),
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
        let alpha = alpha_byte(color);
        if alpha == 0 {
            return Ok(());
        }
        let mut raw_hdc = std::ptr::null_mut();
        if gp::GdipGetDC(graphics, &mut raw_hdc) != 0 || raw_hdc.is_null() {
            return Err(E_FAIL.into());
        }
        let result: WinResult<Option<RenderedTextBitmap>> = (|| {
            let scale = scale.max(0.1);
            let padding = 2i32;
            let full_x = (origin.x * scale).floor() as i32 - padding;
            let full_y = (origin.y * scale).floor() as i32 - padding;
            let full_right = ((origin.x + layout.size.width) * scale).ceil() as i32 + padding;
            let full_bottom = ((origin.y + layout.size.height) * scale).ceil() as i32 + padding;
            let full_width = full_right.saturating_sub(full_x).max(1) as u32;
            let full_height = full_bottom.saturating_sub(full_y).max(1) as u32;
            let (blit_x, blit_y, blit_width, blit_height) =
                clipped_blit(full_x, full_y, full_width, full_height, clip, scale);
            if blit_width <= 0 || blit_height <= 0 {
                return Ok(None);
            }
            let source = HDC(raw_hdc.cast());
            let target = unsafe {
                self.gdi.CreateBitmapRenderTarget(
                    Some(source),
                    blit_width as u32,
                    blit_height as u32,
                )?
            };
            unsafe { target.SetPixelsPerDip(scale)? };
            let identity = DWRITE_MATRIX {
                m11: 1.0,
                m12: 0.0,
                m21: 0.0,
                m22: 1.0,
                dx: 0.0,
                dy: 0.0,
            };
            unsafe { target.SetCurrentTransform(Some(&identity))? };
            let memory = unsafe { target.GetMemoryDC() };
            unsafe {
                BitBlt(
                    memory,
                    0,
                    0,
                    blit_width,
                    blit_height,
                    Some(source),
                    blit_x,
                    blit_y,
                    SRCCOPY,
                )?;
            }
            let context = DrawContext {
                target: &target,
                color: colorref(color),
                scale,
            };
            unsafe {
                layout.layout.Draw(
                    Some((&context as *const DrawContext).cast::<c_void>()),
                    &self.renderer,
                    origin.x - blit_x as f32 / scale,
                    origin.y - blit_y as f32 / scale,
                )?;
                let hbitmap = GetCurrentObject(memory, OBJ_BITMAP);
                if hbitmap.0.is_null() {
                    return Err(E_FAIL.into());
                }
                let mut bitmap: *mut gp::GpBitmap = std::ptr::null_mut();
                if gp::GdipCreateBitmapFromHBITMAP(hbitmap.0, std::ptr::null_mut(), &mut bitmap)
                    != 0
                    || bitmap.is_null()
                {
                    return Err(E_FAIL.into());
                }
                Ok(Some(RenderedTextBitmap {
                    _target: target,
                    bitmap,
                    scale,
                    x: blit_x,
                    y: blit_y,
                    width: blit_width,
                    height: blit_height,
                }))
            }
        })();
        gp::GdipReleaseDC(graphics, raw_hdc);
        let Some(rendered) = result? else {
            return Ok(());
        };

        let mut attributes: *mut gp::GpImageAttributes = std::ptr::null_mut();
        if alpha < u8::MAX {
            if gp::GdipCreateImageAttributes(&mut attributes) != 0 || attributes.is_null() {
                return Err(E_FAIL.into());
            }
            let matrix = opacity_matrix(alpha as f32 / 255.0);
            if gp::GdipSetImageAttributesColorMatrix(
                attributes,
                gp::ColorAdjustTypeDefault,
                1,
                &matrix,
                std::ptr::null(),
                gp::ColorMatrixFlagsDefault,
            ) != 0
            {
                gp::GdipDisposeImageAttributes(attributes);
                return Err(E_FAIL.into());
            }
        }
        let status = gp::GdipDrawImageRectRect(
            graphics,
            rendered.bitmap as *mut gp::GpImage,
            rendered.x as f32 / rendered.scale,
            rendered.y as f32 / rendered.scale,
            rendered.width as f32 / rendered.scale,
            rendered.height as f32 / rendered.scale,
            0.0,
            0.0,
            rendered.width as f32,
            rendered.height as f32,
            gp::UnitPixel,
            attributes,
            0,
            std::ptr::null_mut(),
        );
        if !attributes.is_null() {
            gp::GdipDisposeImageAttributes(attributes);
        }
        if status == 0 {
            Ok(())
        } else {
            Err(E_FAIL.into())
        }
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

fn clipped_blit(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    clip: Option<Rect>,
    scale: f32,
) -> (i32, i32, i32, i32) {
    let right = x.saturating_add(width as i32);
    let bottom = y.saturating_add(height as i32);
    let Some(clip) = clip else {
        return (x, y, width as i32, height as i32);
    };
    let clip_left = (clip.left() * scale).floor() as i32;
    let clip_top = (clip.top() * scale).floor() as i32;
    let clip_right = (clip.right() * scale).ceil() as i32;
    let clip_bottom = (clip.bottom() * scale).ceil() as i32;
    let blit_x = x.max(clip_left);
    let blit_y = y.max(clip_top);
    let blit_right = right.min(clip_right).max(blit_x);
    let blit_bottom = bottom.min(clip_bottom).max(blit_y);
    (blit_x, blit_y, blit_right - blit_x, blit_bottom - blit_y)
}

fn colorref(color: Color) -> COLORREF {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    COLORREF(channel(color.r) | (channel(color.g) << 8) | (channel(color.b) << 16))
}

fn alpha_byte(color: Color) -> u8 {
    (color.a.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn opacity_matrix(alpha: f32) -> gp::ColorMatrix {
    let mut matrix = [0.0; 25];
    matrix[0] = 1.0;
    matrix[6] = 1.0;
    matrix[12] = 1.0;
    matrix[18] = alpha.clamp(0.0, 1.0);
    matrix[24] = 1.0;
    gp::ColorMatrix { m: matrix }
}

struct DrawContext<'a> {
    target: &'a IDWriteBitmapRenderTarget,
    color: COLORREF,
    scale: f32,
}

#[implement(IDWriteTextRenderer)]
struct BitmapTextRenderer;

impl IDWritePixelSnapping_Impl for BitmapTextRenderer_Impl {
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

impl IDWriteTextRenderer_Impl for BitmapTextRenderer_Impl {
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
        unsafe {
            context.target.DrawGlyphRun(
                baseline_x,
                baseline_y,
                measuring_mode,
                glyph_run,
                None::<&IDWriteRenderingParams>,
                context.color,
                None,
            )
        }
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
    fn blit裁剪支持负偏移() {
        assert_eq!(
            clipped_blit(-12, 8, 40, 20, Some(Rect::new(0.0, 10.0, 20.0, 10.0)), 1.0,),
            (0, 10, 20, 10)
        );
    }

    #[test]
    fn blit裁剪按dpi转换为物理像素() {
        assert_eq!(
            clipped_blit(0, 0, 100, 50, Some(Rect::new(10.25, 4.25, 20.0, 10.0)), 2.0,),
            (20, 8, 41, 21)
        );
    }

    #[test]
    fn blit裁剪在无交集时返回空区域() {
        assert_eq!(
            clipped_blit(30, 30, 10, 10, Some(Rect::new(0.0, 0.0, 10.0, 10.0)), 1.0,),
            (30, 30, 0, 0)
        );
    }

    #[test]
    fn 文字透明度转换覆盖边界值() {
        assert_eq!(alpha_byte(Color::rgba(0.0, 0.0, 0.0, -1.0)), 0);
        assert_eq!(alpha_byte(Color::rgba(0.0, 0.0, 0.0, 0.5)), 128);
        assert_eq!(alpha_byte(Color::rgba(0.0, 0.0, 0.0, 2.0)), 255);
    }

    #[test]
    fn 透明度矩阵只改变alpha通道() {
        let matrix = opacity_matrix(0.5);
        assert_eq!(matrix.m[0], 1.0);
        assert_eq!(matrix.m[6], 1.0);
        assert_eq!(matrix.m[12], 1.0);
        assert_eq!(matrix.m[18], 0.5);
        assert_eq!(matrix.m[24], 1.0);
        assert_eq!(matrix.m.iter().filter(|&&value| value != 0.0).count(), 5);
    }
}
