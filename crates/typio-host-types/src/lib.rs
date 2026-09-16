//! Platform-neutral host state and policy types shared by the Typio daemon and
//! its platform integration crates.

pub mod modifiers;
pub mod panel_coordinator;
pub mod panel_present_gate;
pub mod panel_scheduler;
pub mod wayland_pending;

/// Pure input facts recorded from platform events and consumed by the focus
/// controller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputFacts {
    /// At least one activation boundary occurred since the last observation.
    /// Text-only `done` events never clear this edge.
    pub im_focus_changed: bool,
    pub im_done_serial: u32,
    pub im_is_active: bool,
    pub connection_alive: bool,
    pub suspend_gap_detected: bool,
    pub engine_present: bool,
}

bitflags::bitflags! {
    /// Engine-declared host-managed-selection capability flags. Matches the C
    /// constants in `typio/abi/input_context.h`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct HostSelectionFlags: u32 {
        const NAVIGATE   = 1 << 0;
        const COMMIT     = 1 << 1;
        const INDEX_PICK = 1 << 2;
        const COMMIT_RAW = 1 << 3;
    }
}

/// Typeface family class for the panel text.
///
/// This is the real capability boundary of the text stack: flux-text resolves
/// faces through fontconfig and exposes a *family class* selector, not a
/// free-form family name. Modelling the setting as a class keeps the
/// user-facing option honest — every accepted value actually changes the
/// rendered typeface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontFamilyClass {
    /// Let fontconfig pick (its own sans-serif preference list).
    #[default]
    Default,
    /// Sans-serif preference list.
    Sans,
    /// Serif preference list.
    Serif,
    /// Monospace preference list.
    Mono,
}

impl FontFamilyClass {
    /// Parse the config-file spelling of a family class.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "default" => Some(Self::Default),
            "sans" | "sans-serif" => Some(Self::Sans),
            "serif" => Some(Self::Serif),
            "mono" | "monospace" => Some(Self::Mono),
            _ => None,
        }
    }

    /// The canonical config-file spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Sans => "sans",
            Self::Serif => "serif",
            Self::Mono => "mono",
        }
    }
}

/// Font configuration consumed by the candidate panel renderer.
#[derive(Clone, Debug)]
pub struct PanelFontConfig {
    /// User-configured family class.
    pub family: FontFamilyClass,
    /// Font size in points, clamped by the host config loader.
    pub size_pt: f64,
}

impl Default for PanelFontConfig {
    fn default() -> Self {
        Self {
            family: FontFamilyClass::default(),
            size_pt: 11.0,
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
        (self.size_pt * (96.0 / 72.0)) as f32
    }

    /// Candidate index-number size in logical pixels (one step smaller).
    pub fn number_size_px(&self) -> f32 {
        (self.size_pt * 0.69 * (96.0 / 72.0)) as f32
    }

    /// Status-banner text size in logical pixels.
    pub fn banner_size_px(&self) -> f32 {
        (self.size_pt * 0.94 * (96.0 / 72.0)) as f32
    }
}

// Flat re-exports: every host type lives at the crate root so callers do not
// have to track which internal module a type was moved to.
pub use modifiers::*;
pub use panel_coordinator::*;
pub use panel_present_gate::*;
pub use panel_scheduler::*;
pub use wayland_pending::*;
