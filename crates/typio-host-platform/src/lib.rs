//! Wayland/Flux platform integration for the Typio daemon.
//!
//! This crate owns the effectful compositor-facing pieces: protocol bindings,
//! the input-method frontend, SHM-backed panel presentation, and Flux text
//! rendering. Platform-neutral policy lives in `typio-host-types`.

pub mod input_method;
pub mod panel;
pub mod panel_shm;
pub mod protocols;
pub mod text_raster;

pub use typio_host_types::{
    FontFamilyClass, HostSelectionFlags, InputFacts, Modifiers, PanelFontConfig, panel_coordinator,
    panel_present_gate, panel_scheduler, wayland_pending,
};
