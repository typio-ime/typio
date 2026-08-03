//! Wayland keyboard subsystem.
//!
//! The router bridges input-method keyboard grabs to typio-core's input
//! context. Pure policy helpers (modifiers, chords, repeat guard, tracker)
//! live in [`crate::keyboard_policy`].

mod helpers;
mod output;
mod preedit_coalescer;
pub mod router;
