//! Platform-neutral host state and policy types shared by typio-host and its
//! platform integration crates.

pub mod panel_coordinator;
pub mod panel_present_gate;
pub mod panel_scheduler;
pub mod text_serial_gate;
pub mod wayland_pending;

/// Pure input facts recorded from platform events and consumed by the focus
/// controller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputFacts {
    pub im_activate_seen: bool,
    pub im_deactivate_seen: bool,
    pub im_done_had_activate: bool,
    pub im_done_had_deactivate: bool,
    pub im_done_serial: u32,
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

/// Font configuration consumed by the candidate panel renderer.
#[derive(Clone, Debug)]
pub struct PanelFontConfig {
    /// User-configured primary family, or empty for built-in fallback.
    pub family: String,
    /// Font size in points, clamped by the host config loader.
    pub size_pt: f64,
}

impl Default for PanelFontConfig {
    fn default() -> Self {
        Self {
            family: String::new(),
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

    /// The configured family, or `None` when empty (pure fallback selection).
    pub fn family_opt(&self) -> Option<String> {
        if self.family.is_empty() {
            None
        } else {
            Some(self.family.clone())
        }
    }
}

// Compatibility re-exports for modules moved out of typio-host.
pub use panel_coordinator::*;
pub use panel_present_gate::*;
pub use panel_scheduler::*;
pub use text_serial_gate::*;
pub use wayland_pending::*;
