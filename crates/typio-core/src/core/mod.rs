//! Engine subsystem (ADR-0005).
//!
//! Pure-Rust registry and process-backed engine client. `EngineRegistry` owns
//! concrete out-of-process backends. The typed Engine Protocol is the only
//! engine boundary (ADR-0046).

pub mod engine;
pub mod registry;
