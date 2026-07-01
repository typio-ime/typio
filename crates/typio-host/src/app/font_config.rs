//! Panel font configuration read from libtypio's frontend-owned config keys
//! `display.font_family` and `display.font_size`.
//!
//! These keys are intentionally *not* statically registered in typio-core's
//! schema (see `display_fields_are_not_built_in`): they are owned by the
//! frontend. We read them through the same C-ABI getters used for the
//! indicator config, snapshotting once at startup and on every reload so the
//! render hot path never touches FFI.

use std::ffi::{c_char, CStr, CString};

use typio::TypioInstance;

use super::App;

/// Default font family: empty means "let the rasteriser's built-in fallback
/// lists choose" (Noto Sans / Noto Sans CJK SC / …).
pub(crate) const DEFAULT_FONT_FAMILY: &str = "";
/// Default font size in points, matching the documented range 6–72.
pub(crate) const DEFAULT_FONT_SIZE_PT: f64 = 11.0;
/// Documented clamp range for `display.font_size`.
const FONT_SIZE_MIN_PT: f64 = 6.0;
const FONT_SIZE_MAX_PT: f64 = 72.0;

/// Pixel-per-point factor. The panel sizes everything in physical pixels; a
/// point maps to 96/72 logical px (CSS point), then the HiDPI scale is applied
/// separately at draw time.
const PT_TO_PX: f64 = 96.0 / 72.0;

/// Snapshot of the panel font configuration (logical units; the HiDPI scale is
/// applied at draw time).
#[derive(Clone, Debug)]
pub struct PanelFontConfig {
    /// User-configured primary family, or empty for built-in fallback.
    pub family: String,
    /// Font size in points, clamped to 6–72.
    pub size_pt: f64,
}

impl Default for PanelFontConfig {
    fn default() -> Self {
        Self {
            family: DEFAULT_FONT_FAMILY.to_string(),
            size_pt: DEFAULT_FONT_SIZE_PT,
        }
    }
}

impl PartialEq for PanelFontConfig {
    fn eq(&self, other: &Self) -> bool {
        self.family == other.family && (self.size_pt - other.size_pt).abs() < 0.01
    }
}

impl PanelFontConfig {
    /// Candidate/main text size in logical pixels.
    pub fn candidate_size_px(&self) -> f32 {
        (self.size_pt * PT_TO_PX) as f32
    }

    /// Candidate index-number size in logical pixels (one step smaller).
    pub fn number_size_px(&self) -> f32 {
        (self.size_pt * 0.69 * PT_TO_PX) as f32
    }

    /// Status-banner text size in logical pixels.
    pub fn banner_size_px(&self) -> f32 {
        (self.size_pt * 0.94 * PT_TO_PX) as f32
    }

    /// The configured family, or `None` when empty (pure fallback selection).
    pub fn family_opt(&self) -> Option<String> {
        if self.family.is_empty() {
            None
        } else {
            Some(self.family.clone())
        }
    }
}

impl App {
    /// Read `display.font_family` / `display.font_size` from libtypio's config
    /// cache. Returns defaults when the instance or config pointer is
    /// unavailable. Mirrors [`App::load_indicator_config`].
    pub(super) fn load_display_font_config(&self) -> PanelFontConfig {
        let raw = match self.instance.as_ref() {
            Some(i) => i.as_ref() as *const TypioInstance as *mut TypioInstance,
            None => return PanelFontConfig::default(),
        };
        let cfg = typio::instance::typio_instance_get_config(raw);
        if cfg.is_null() {
            return PanelFontConfig::default();
        }

        let family = get_string(
            cfg,
            "display.font_family",
            DEFAULT_FONT_FAMILY,
        );

        let size_pt = typio::config::typio_config_get_float(
            cfg,
            c"display.font_size".as_ptr(),
            DEFAULT_FONT_SIZE_PT,
        )
        .clamp(FONT_SIZE_MIN_PT, FONT_SIZE_MAX_PT);

        PanelFontConfig { family, size_pt }
    }
}

/// Read a string config key into an owned `String`, falling back to `default`.
fn get_string(cfg: *const typio::config::Config, key: &str, default: &str) -> String {
    let key_c = CString::new(key).unwrap();
    let default_c = CString::new(default).unwrap();
    let ptr: *const c_char = typio::config::typio_config_get_string(
        cfg,
        key_c.as_ptr(),
        default_c.as_ptr(),
    );
    if ptr.is_null() {
        return default.to_string();
    }
    // SAFETY: libtypio returns a pointer into the config's `String` storage
    // (or our `default_c`, which is alive for this scope). Read-only borrow.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
