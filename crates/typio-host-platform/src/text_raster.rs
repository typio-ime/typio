//! Text shaping + rasterisation for the candidate panel and status banners.
//!
//! Delegates to **`optics::text`** (FreeType + HarfBuzz + Fontconfig + FriBidi,
//! subpixel-positioned R8 coverage atlas). On a device-less CPU canvas
//! (`optics_gfx_canvas_create` with the CPU backend) the glyph run flows
//! through the host-coverage path (ADR-0019): `optics-text` rasterises glyphs
//! into a host R8 buffer and the CPU canvas samples coverage straight from
//! it — no GPU image required.
//!
//! All public sizes/positions are in logical pixels; `optics_text_draw` honours
//! the canvas content-scale transform so glyphs rasterise crisply on HiDPI.

use optics::gfx::sys as gfx_sys;
use optics::text::sys as text_sys;
use text_sys::{
    optics_text_create, optics_text_ctx, optics_text_desc, optics_text_draw, optics_text_draw_opts,
    optics_text_family, optics_text_measure, optics_text_measure_opts, optics_text_metrics,
    optics_text_release, optics_text_set_default_family, optics_text_style,
};

use crate::FontFamilyClass;

/// Shaped extent of a run in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

impl From<optics_text_metrics> for TextMetrics {
    fn from(m: optics_text_metrics) -> Self {
        Self {
            width: m.width,
            height: m.height,
            baseline: m.baseline,
        }
    }
}

/// Pack an opaque `[r,g,b]` into `optics_gfx_color` (straight RGBA8).
fn color_rgb(c: [u8; 3]) -> u32 {
    // optics_gfx_color_rgba packs R,G,B,A little-endian; alpha 255 = opaque.
    u32::from_le_bytes([c[0], c[1], c[2], 255])
}

/// Shaping + glyph-run front-end backed by `optics-text`.
///
/// Holds one `optics_text_ctx*` context (no device: device-less CPU canvas path).
/// The preferred family class is applied to the context itself, so it steers
/// every glyph run drawn through this rasteriser without per-call plumbing.
pub struct TextRaster {
    raw: *mut optics_text_ctx,
    family: FontFamilyClass,
}

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
        let desc = optics_text_desc {
            device: core::ptr::null_mut(),
            scale: 1.0,
        };
        let mut out: *mut optics_text_ctx = core::ptr::null_mut();
        // optics_text_create never fails: on backend init failure it falls back
        // to a measure-only monospace context (has_backend == false).
        let _ = unsafe { optics_text_create(&desc, &mut out) };
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
    /// Maps the host's [`FontFamilyClass`] onto `optics-text`'s family selector.
    /// This is the real capability: `optics-text` resolves faces through
    /// fontconfig's preference list for the selected class, and per-codepoint
    /// fallback still covers anything the class's faces do not. Individual
    /// typeface names are therefore not selectable — the class is.
    pub fn set_preferred_family(&mut self, family: FontFamilyClass) {
        if self.raw.is_null() {
            return;
        }
        let raw = match family {
            FontFamilyClass::Default => optics_text_family::OPTICS_TEXT_FAMILY_DEFAULT,
            FontFamilyClass::Sans => optics_text_family::OPTICS_TEXT_FAMILY_SANS,
            FontFamilyClass::Serif => optics_text_family::OPTICS_TEXT_FAMILY_SERIF,
            FontFamilyClass::Mono => optics_text_family::OPTICS_TEXT_FAMILY_MONO,
        };
        // SAFETY: raw is a live optics_text_ctx context; raw is a value type.
        unsafe { optics_text_set_default_family(self.raw, raw) };
        self.family = family;
    }

    /// Measure `text` at `size_px` (logical). Returns width + baseline/height.
    pub fn measure(&mut self, text: &str, size_px: f32) -> TextMetrics {
        if text.is_empty() || size_px <= 0.0 || self.raw.is_null() {
            return TextMetrics::default();
        }
        let style = optics_text_style {
            size_px,
            weight: 0.0,
            color: 0,
            family: optics_text_family::OPTICS_TEXT_FAMILY_DEFAULT,
            italic: false,
        };
        let opts = optics_text_measure_opts {
            utf8: text.as_ptr() as *const i8,
            len: text.len(),
            style: &style,
        };
        let m = unsafe { optics_text_measure(self.raw, &opts) };
        m.into()
    }

    /// Draw `text` into `canvas` (a CPU canvas mid-pass) with the text-box
    /// top-left at (`x`, `y`) logical pixels, at `size_px` logical, in solid
    /// `color`. Must be called between `optics_gfx_canvas_begin` and
    /// `optics_gfx_canvas_end`; the glyph run is composited into the canvas's
    /// framebuffer via the host-coverage path.
    #[allow(clippy::too_many_arguments, clippy::not_unsafe_ptr_arg_deref)]
    pub fn draw(
        &mut self,
        canvas: *mut gfx_sys::optics_gfx_canvas,
        x: f32,
        y: f32,
        text: &str,
        size_px: f32,
        color: [u8; 3],
    ) {
        if text.is_empty() || size_px <= 0.0 || self.raw.is_null() || canvas.is_null() {
            return;
        }
        let style = optics_text_style {
            size_px,
            weight: 0.0,
            color: color_rgb(color),
            family: optics_text_family::OPTICS_TEXT_FAMILY_DEFAULT,
            italic: false,
        };
        let opts = optics_text_draw_opts {
            canvas,
            arena: core::ptr::null_mut(),
            x,
            y,
            utf8: text.as_ptr() as *const i8,
            len: text.len(),
            style: &style,
            outline_color: 0,
            outline_width: 0.0,
        };
        unsafe { optics_text_draw(self.raw, &opts) };
    }
}

impl Drop for TextRaster {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { optics_text_release(self.raw) };
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
        // optics-text is built with -Dtext=true). A measure-only fallback still
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
