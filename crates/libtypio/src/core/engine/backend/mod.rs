//! Transport abstraction: `EngineBackend` is the registry's only view of an engine.
//!
//! `EngineRegistry` never inspects the transport. It only sees `EngineBackend`.
//!
//! `EngineBackend` is an **enum**, not a trait, to avoid dyn-compatibility
//! issues with generic methods like `with_engine`. There is intentionally one
//! transport in the long-term model: every engine runs out of process.

pub mod engine_protocol;
pub mod process;

use super::{Engine, EngineInfo, Result};

/// Transport-layer abstraction for an engine.
///
/// Variants:
/// - `Process`: out-of-process engine transport.
#[derive(Debug)]
pub enum EngineBackend {
    /// Out-of-process engine process transport.
    Process(process::ProcessBackend),
}

impl EngineBackend {
    /// Immutable metadata. Available without instantiating.
    pub fn info(&self) -> &EngineInfo {
        match self {
            EngineBackend::Process(b) => b.info(),
        }
    }

    /// Mutable metadata, for registry-side enrichment (languages).
    pub(crate) fn info_mut(&mut self) -> &mut EngineInfo {
        match self {
            EngineBackend::Process(b) => b.info_mut(),
        }
    }

    /// Lazily create or connect to the engine instance.
    pub fn instantiate(&mut self) -> Result<()> {
        match self {
            EngineBackend::Process(b) => b.instantiate(),
        }
    }

    /// Whether `instantiate()` has already succeeded.
    pub fn is_instantiated(&self) -> bool {
        match self {
            EngineBackend::Process(b) => b.is_instantiated(),
        }
    }

    /// Execute a closure with a mutable reference to the engine, if available.
    ///
    /// Returns `None` if `instantiate()` has not been called or failed.
    pub fn with_engine<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn Engine) -> R,
    {
        match self {
            EngineBackend::Process(b) => b.with_engine(f),
        }
    }

    /// Execute a closure with an immutable reference to the engine, if available.
    pub fn with_engine_ref<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&dyn Engine) -> R,
    {
        match self {
            EngineBackend::Process(b) => b.with_engine_ref(f),
        }
    }

    /// Tear down the engine and release transport resources.
    ///
    /// Safe to call multiple times; idempotent.
    pub fn destroy(&mut self) {
        match self {
            EngineBackend::Process(b) => b.destroy(),
        }
    }
}

/// A factory that can create `EngineBackend` instances.
///
/// Hosts use this to register engines without exposing construction details
/// to core.
pub trait BackendFactory: Send + Sync {
    /// Create a new backend instance for the given engine info.
    fn create_backend(&self, info: EngineInfo) -> EngineBackend;
}
