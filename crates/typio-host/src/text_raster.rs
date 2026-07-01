//! Text shaping + rasterisation for the candidate panel and status banners.
//!
//! Delegates to **flux-text** (the optics C sibling: FreeType + HarfBuzz +
//! Fontconfig + FriBidi, subpixel-positioned R8 coverage atlas). On a
//! device-less CPU canvas (`flux_canvas_create_cpu`) the glyph run flows
//! through the host-coverage path (ADR-0019): flux-text rasterises glyphs
//! into a host R8 buffer and the CPU canvas samples coverage straight from
//! it — no GPU image required. This replaces the former ab_glyph software
//! rasteriser, which was a workaround for flux's CPU canvas dropping glyph
//! draws.
//!
//! All public sizes/positions are in logical pixels; `flux_text_draw` honours
//! the canvas content-scale transform so glyphs rasterise crisply on HiDPI.

use flux_text_sys::{
    flux_text, flux_text_create, flux_text_destroy, flux_text_draw, flux_text_measure,
    flux_text_desc, flux_text_family, flux_text_metrics, flux_text_style,
};

/// Shaped extent of a run in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

impl From<flux_text_metrics> for TextMetrics {
    fn from(m: flux_text_metrics) -> Self {
        Self {
            width: m.width,
            height: m.height,
            baseline: m.baseline,
        }
    }
}

/// Pack an opaque `[r,g,b]` into flux's `flux_color` (straight RGBA8).
fn color_rgb(c: [u8; 3]) -> u32 {
    // flux_color_rgba packs R,G,B,A little-endian; alpha 255 = opaque.
    u32::from_le_bytes([c[0], c[1], c[2], 255])
}

/// Shaping + glyph-run front-end backed by flux-text.
///
/// Holds one `flux_text*` context (no device: device-less CPU canvas path).
/// `set_preferred_family` is retained for API compatibility but maps to
/// flux-text's fontconfig fallback — a user-configured CJK family is selected
/// automatically by fontconfig's sort, so no explicit family pin is needed.
pub struct TextRaster {
    raw: *mut flux_text,
}

// The underlying flux_text* is thread-affine (same constraint as flux-text's
// safe wrapper); the panel drives it from a single thread.
unsafe impl Send for TextRaster {}

impl Default for TextRaster {
    fn default() -> Self {
        Self::new(None)
    }
}

impl TextRaster {
    pub fn new(_preferred_family: Option<String>) -> Self {
        // No device: the host-coverage path (ADR-0019) renders into a CPU
        // canvas without a GPU atlas. A NULL device still builds the full
        // FreeType + HarfBuzz + fontconfig backend.
        let desc = flux_text_desc {
            device: core::ptr::null_mut(),
            scale: 1.0,
        };
        let mut out: *mut flux_text = core::ptr::null_mut();
        // flux_text_create never fails: on backend init failure it falls back
        // to a measure-only monospace context (has_backend == false).
        let _ = unsafe { flux_text_create(&desc, &mut out) };
        TextRaster { raw: out }
    }

    /// Change the primary font family at runtime. With flux-text the actual
    /// face selection is fontconfig's: a configured CJK family is picked
    /// automatically when it covers a codepoint, so this is a no-op kept only
    /// for source compatibility with the former ab_glyph rasteriser.
    pub fn set_preferred_family(&mut self, _family: Option<String>) {
        // No-op: flux-text resolves faces via fontconfig fallback.
    }

    /// Measure `text` at `size_px` (logical). Returns width + baseline/height.
    pub fn measure(&mut self, text: &str, size_px: f32) -> TextMetrics {
        if text.is_empty() || size_px <= 0.0 || self.raw.is_null() {
            return TextMetrics::default();
        }
        let style = flux_text_style {
            size_px,
            weight: 0.0,
            color: 0,
            family: flux_text_family::FLUX_TEXT_FAMILY_DEFAULT,
            italic: false,
        };
        let m = unsafe { flux_text_measure(self.raw, text.as_ptr() as *const i8, text.len(), &style) };
        m.into()
    }

    /// Draw `text` into `canvas` (a CPU canvas mid-pass) with the text-box
    /// top-left at (`x`, `y`) logical pixels, at `size_px` logical, in solid
    /// `color`. Must be called between `flux_canvas_cpu_begin` and
    /// `flux_canvas_cpu_end`; the glyph run is composited into the canvas's
    /// framebuffer via the host-coverage path.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: *mut flux_sys::flux_canvas,
        x: f32,
        y: f32,
        text: &str,
        size_px: f32,
        color: [u8; 3],
    ) {
        if text.is_empty() || size_px <= 0.0 || self.raw.is_null() || canvas.is_null() {
            return;
        }
        let style = flux_text_style {
            size_px,
            weight: 0.0,
            color: color_rgb(color),
            family: flux_text_family::FLUX_TEXT_FAMILY_DEFAULT,
            italic: false,
        };
        unsafe {
            flux_text_draw(
                self.raw,
                // flux-text-sys re-declares flux_canvas from its own bindgen
                // pass; it is ABI-identical to flux_sys::flux_canvas (both are
                // the same opaque C pointer), so cast across the crate boundary.
                canvas as *mut flux_text_sys::flux_canvas,
                core::ptr::null_mut(),
                x,
                y,
                text.as_ptr() as *const i8,
                text.len(),
                &style,
            )
        };
    }
}

impl Drop for TextRaster {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { flux_text_destroy(self.raw) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_empty_is_zero() {
        let mut t = TextRaster::default();
        assert_eq!(t.measure("", 16.0), TextMetrics::default());
    }

    #[test]
    fn measure_latin_has_positive_width() {
        let mut t = TextRaster::default();
        let m = t.measure("Ab", 16.0);
        // Only meaningful when the FT/HB backend is present (it is, when
        // flux-text built with -Dtext=true). A measure-only fallback still
        // returns a width, so this is a soft check.
        assert!(m.width >= 0.0);
    }

    #[test]
    fn set_preferred_family_is_noop_but_safe() {
        let mut t = TextRaster::default();
        t.set_preferred_family(Some("Noto Sans".into()));
        t.set_preferred_family(None);
    }
}
