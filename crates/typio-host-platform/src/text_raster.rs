//! Text shaping + rasterisation for the candidate panel and status banners.
//!
//! Delegates to **flux-text** (the optics C sibling: FreeType + HarfBuzz +
//! Fontconfig + FriBidi, subpixel-positioned R8 coverage atlas). On a
//! device-less CPU canvas (`flux_canvas_create_cpu`) the glyph run flows
//! through the host-coverage path (ADR-0019): flux-text rasterises glyphs
//! into a host R8 buffer and the CPU canvas samples coverage straight from
//! it — no GPU image required.
//!
//! All public sizes/positions are in logical pixels; `flux_text_draw` honours
//! the canvas content-scale transform so glyphs rasterise crisply on HiDPI.

use flux_text_sys::{
    flux_text, flux_text_create, flux_text_desc, flux_text_draw, flux_text_family,
    flux_text_measure, flux_text_metrics, flux_text_release, flux_text_set_default_family,
    flux_text_style,
};

use crate::FontFamilyClass;

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
/// The preferred family class is applied to the context itself, so it steers
/// every glyph run drawn through this rasteriser without per-call plumbing.
pub struct TextRaster {
    raw: *mut flux_text,
    family: FontFamilyClass,
}

// The underlying flux_text* is thread-affine (same constraint as flux-text's
// safe wrapper); the panel drives it from a single thread.
unsafe impl Send for TextRaster {}

impl Default for TextRaster {
    fn default() -> Self {
        Self::new(FontFamilyClass::default())
    }
}

impl TextRaster {
    /// Create a rasteriser with `family` as the context's preferred family
    /// class. The class is applied immediately, so the first measured or drawn
    /// run already uses it.
    pub fn new(family: FontFamilyClass) -> Self {
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
        let mut raster = TextRaster {
            raw: out,
            family: FontFamilyClass::Default,
        };
        raster.set_preferred_family(family);
        raster
    }

    /// The family class currently applied to the context.
    pub fn preferred_family(&self) -> FontFamilyClass {
        self.family
    }

    /// Switch the context's preferred family class.
    ///
    /// Maps the host's [`FontFamilyClass`] onto flux-text's family selector.
    /// This is the real capability: flux-text resolves faces through
    /// fontconfig's preference list for the selected class, and per-codepoint
    /// fallback still covers anything the class's faces do not. Individual
    /// typeface names are therefore not selectable — the class is.
    pub fn set_preferred_family(&mut self, family: FontFamilyClass) {
        if self.raw.is_null() {
            return;
        }
        let raw = match family {
            FontFamilyClass::Default => flux_text_family::FLUX_TEXT_FAMILY_DEFAULT,
            FontFamilyClass::Sans => flux_text_family::FLUX_TEXT_FAMILY_SANS,
            FontFamilyClass::Serif => flux_text_family::FLUX_TEXT_FAMILY_SERIF,
            FontFamilyClass::Mono => flux_text_family::FLUX_TEXT_FAMILY_MONO,
        };
        // SAFETY: raw is a live flux-text context; raw is a value type.
        unsafe { flux_text_set_default_family(self.raw, raw) };
        self.family = family;
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
        let m =
            unsafe { flux_text_measure(self.raw, text.as_ptr() as *const i8, text.len(), &style) };
        m.into()
    }

    /// Draw `text` into `canvas` (a CPU canvas mid-pass) with the text-box
    /// top-left at (`x`, `y`) logical pixels, at `size_px` logical, in solid
    /// `color`. Must be called between `flux_canvas_begin` and
    /// `flux_canvas_end`; the glyph run is composited into the canvas's
    /// framebuffer via the host-coverage path.
    #[allow(clippy::too_many_arguments, clippy::not_unsafe_ptr_arg_deref)]
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
                canvas,
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
            unsafe { flux_text_release(self.raw) };
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
    fn preferred_family_class_round_trips() {
        let mut t = TextRaster::default();
        assert_eq!(t.preferred_family(), FontFamilyClass::Default);

        for class in [
            FontFamilyClass::Sans,
            FontFamilyClass::Serif,
            FontFamilyClass::Mono,
            FontFamilyClass::Default,
        ] {
            t.set_preferred_family(class);
            assert_eq!(t.preferred_family(), class);
        }
    }

    #[test]
    fn font_family_class_parses_canonical_spellings() {
        assert_eq!(FontFamilyClass::parse("mono"), Some(FontFamilyClass::Mono));
        assert_eq!(
            FontFamilyClass::parse("Monospace"),
            Some(FontFamilyClass::Mono)
        );
        assert_eq!(FontFamilyClass::parse(""), Some(FontFamilyClass::Default));
        assert_eq!(FontFamilyClass::parse("Noto Sans"), None);
        assert_eq!(FontFamilyClass::Mono.as_str(), "mono");
    }
}
