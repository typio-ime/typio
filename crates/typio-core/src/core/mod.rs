//! Engine subsystem (ADR-0005).
//!
//! Pure-Rust trait system backed by the `EngineBackend` abstraction. FFI
//! and process-backed engines are implementations of `EngineBackend`; `EngineRegistry`
//! never distinguishes between them.
//!
//! The C ABI surface (ADR-0002) is provided via a thin translation layer
//! in `c_api/` that calls into `core::registry::EngineRegistry`.

pub mod engine;
pub mod registry;
