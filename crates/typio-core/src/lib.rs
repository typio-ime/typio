//! typio-core — Rust implementation of the Typio input method framework library.
//!
//! This crate exports a C ABI compatible with `include/typio/*.h`.
//! This is the primary implementation; there is no separate C runtime.

// This crate is a C-ABI boundary: almost every public function is a
// `#[unsafe(no_mangle)] extern "C"` entry point that dereferences raw pointers
// passed in by the C caller. Converting every export and callback vtable to
// `unsafe extern "C"` is a separate Rust-API migration, so the raw-pointer lint
// is allowed at this boundary while the ABI remains pre-1.0. The safety
// contract lives at the call sites in the host and native engine workers.
#![allow(unsafe_op_in_unsafe_fn, clippy::not_unsafe_ptr_arg_deref)]
#![warn(missing_docs)]

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
/// Voice related operations
pub mod voice;
/// RAII wrappers for C pointers
pub mod wrappers;

// Re-export at crate root so cbindgen can see them easily
pub use config::*;
pub use config_schema::*;
pub use event::*;
pub use input_context::*;
pub use instance::*;
pub use log::*;
pub use string::*;
pub use types::*;

#[cfg(test)]
mod integration_tests;
