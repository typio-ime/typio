//! Panel font configuration read from the frontend-owned config keys
//! `display.font_family` and `display.font_size`.
//!
//! These keys are intentionally *not* statically registered in the runtime
//! config schema (see `display_fields_belong_to_frontends`): they are owned by
//! the frontend. We read them through the same owned config API used for the
//! indicator config, snapshotting once at startup and on every reload so the
//! render hot path never touches configuration storage.

use super::App;

/// Default font size in points, matching the documented range 6–72.
pub(crate) const DEFAULT_FONT_SIZE_PT: f64 = 11.0;
/// Documented clamp range for `display.font_size`.
const FONT_SIZE_MIN_PT: f64 = 6.0;
const FONT_SIZE_MAX_PT: f64 = 72.0;

pub use typio_host_types::{FontFamilyClass, PanelFontConfig};

impl App {
    /// Read `display.font_family` / `display.font_size` from the runtime's
    /// config cache. Returns defaults when the instance or config is
    /// unavailable. Mirrors [`App::load_indicator_config`].
    pub(super) fn load_display_font_config(&self) -> PanelFontConfig {
        let instance = match self.instance.as_ref() {
            Some(instance) => instance.borrow(),
            None => return PanelFontConfig::default(),
        };
        let Some(config) = instance.config_rust() else {
            return PanelFontConfig::default();
        };

        let raw_family = config.string("display.font_family", "default");
        let family = FontFamilyClass::parse(raw_family).unwrap_or_else(|| {
            tracing::warn!(
                target: "typio.config",
                value = raw_family,
                "display.font_family is not a known family class \
                 (expected one of: default, sans, serif, mono); using default"
            );
            FontFamilyClass::Default
        });

        let size_pt = config
            .float("display.font_size", DEFAULT_FONT_SIZE_PT)
            .clamp(FONT_SIZE_MIN_PT, FONT_SIZE_MAX_PT);

        PanelFontConfig { family, size_pt }
    }
}
