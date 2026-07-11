//! Wayland keyboard subsystem.
//!
//! The router bridges input-method keyboard grabs to libtypio's input
//! context. Pure policy helpers (modifiers, chords, repeat guard, tracker)
//! live in [`crate::keyboard_policy`].

mod ffi_callbacks;
mod helpers;
mod preedit_coalescer;
pub mod router;
