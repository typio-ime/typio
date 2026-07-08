//! Engine subsystem (ADR-0005).
//!
//! Pure-Rust registry and process-backed engine client. `EngineRegistry` owns
//! concrete out-of-process backends; in-process FFI engines are no longer a
//! runtime model.
//!
//! The C ABI surface (ADR-0002) is provided via a thin translation layer
//! in `c_api/` that calls into `core::registry::EngineRegistry`.

pub mod engine;
pub mod registry;
