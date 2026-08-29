//! Wayland/Flux platform integration for typio-host.
//!
//! This crate owns the effectful compositor-facing pieces: protocol bindings,
//! the input-method frontend, SHM-backed panel presentation, and Flux text
//! rendering. Pure host policy remains in typio-host/typio-host-types.

// Low-level Wayland/Flux integration still uses FFI wrappers. Keep the Rust
// 2024 edition migration focused on edition compatibility, not an unsafe block
// audit.
#![allow(unsafe_op_in_unsafe_fn)]

pub mod input_method;
pub mod panel;
pub mod panel_shm;
pub mod protocols;
pub mod text_raster;

pub use typio_host_types::{
    HostSelectionFlags, InputFacts, Modifiers, PanelFontConfig, panel_coordinator,
    panel_present_gate, panel_scheduler, wayland_pending,
};
