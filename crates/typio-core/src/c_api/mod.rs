//! C ABI boundary layer (ADR-0005).
//!
//! Thin translation layer between C callers and the Rust-native
//! `core::registry`. `typio_registry_*` is the sole engine-management C ABI.

pub mod registry;
