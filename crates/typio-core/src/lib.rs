//! typio-core — Rust implementation of the Typio input method framework library.
//!
//! This crate exports a C ABI compatible with `include/typio/*.h`.
//! This is the primary implementation; there is no separate C runtime.

// This crate is a C-ABI boundary: almost every public function is a
// `#[no_mangle] extern "C"` entry point that dereferences raw pointers passed
// in by the C caller. Marking them all `unsafe` does not change the C-callable
// signature and only adds noise, so the lint is allowed crate-wide. The safety
// contract lives at the call sites in `daemon/` and the engine plugins.
#![warn(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod c_api;
pub mod config;
pub mod config_schema;
pub mod core;
pub mod engine;
pub mod event;
pub mod input_context;
pub mod instance;
pub mod log;
pub mod shortcut;
pub mod string;
pub mod types;
/// Voice input and processing.
pub mod voice;

// Re-export at crate root so cbindgen can see them easily
pub use config::*;
pub use config_schema::*;
pub use engine::*;
pub use event::*;
pub use input_context::*;
pub use instance::*;
pub use log::*;
pub use string::*;
pub use types::*;

#[cfg(test)]
mod integration_tests;
