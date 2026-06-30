//! Pure-Rust engine trait hierarchy (ADR-0005).
//!
//! This is the sole internal interface for all engines. No C vtables,
//! no raw pointers, no `unsafe` required to implement an engine.

use std::fmt;

pub mod backend;
pub mod event;
pub mod mode;

pub use event::{KeyEvent, KeyProcessResult, KeyState, KeySym};
pub use mode::{EngineCapabilities, EngineMode, ModeSalience};

/// Unified result type for engine operations.
pub type Result<T> = std::result::Result<T, EngineError>;

/// Engine subsystem errors.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    /// Argument was invalid or missing.
    InvalidArgument,
    /// Requested resource was not found.
    NotFound,
    /// Resource already exists.
    AlreadyExists,
    /// Engine failed to load (e.g. missing data files).
    LoadFailed(String),
    /// Transport error.
    Transport(String),
    /// Operation not supported by this engine.
    NotSupported,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::InvalidArgument => write!(f, "invalid argument"),
            EngineError::NotFound => write!(f, "not found"),
            EngineError::AlreadyExists => write!(f, "already exists"),
            EngineError::LoadFailed(s) => write!(f, "load failed: {s}"),
            EngineError::Transport(s) => write!(f, "transport error: {s}"),
            EngineError::NotSupported => write!(f, "not supported"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Engine category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineType {
    /// Keyboard input engine (e.g. pinyin, mozc).
    Keyboard,
    /// Voice / speech-to-text engine.
    Voice,
}

/// Guidance for the host when choosing a backend for an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendPreference {
    /// Prefer in-process FFI. Default for keyboard engines.
    FfiPreferred,
    /// Prefer an out-of-process engine. Default for voice / AI engines.
    ProcessPreferred,
    /// Must be in-process (e.g. requires direct GPU memory access).
    FfiOnly,
    /// Must be out-of-process (e.g. untrusted third-party code).
    ProcessOnly,
}

/// Engine availability lifecycle axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineAvailability {
    /// The engine has been created but initialization has not completed.
    Uninitialized,
    /// The engine is performing asynchronous warm-up and must not receive input.
    Preparing,
    /// The engine can process input.
    Ready,
    /// The engine failed warm-up and must not receive input.
    Failed,
}

/// Immutable engine metadata.
#[derive(Debug, Clone)]
pub struct EngineInfo {
    /// Machine-readable engine identifier (e.g. "rime").
    pub name: String,
    /// Human-readable name (e.g. "Rime").
    pub display_name: String,
    /// Short description of the engine.
    pub description: String,
    /// Engine author or vendor.
    pub author: String,
    /// Freedesktop icon name or file name (no paths or URLs).
    pub icon: Option<String>,
    /// BCP-47 language tag (default "und").
    pub language: String,
    /// Ordered BCP-47 language tags the engine supports, primary first.
    /// Empty means "exactly `language`". The tag `"mul"` declares support
    /// for every language.
    pub languages: Vec<String>,
    /// Keyboard or voice.
    pub engine_type: EngineType,
    /// Capabilities bitmask.
    pub capabilities: EngineCapabilities,
    /// Host backend preference hint.
    pub backend_preference: BackendPreference,
}

impl EngineInfo {
    /// Create a minimal `EngineInfo` with defaults for all other fields.
    pub fn new(name: impl Into<String>, engine_type: EngineType) -> Self {
        let name = name.into();
        Self {
            display_name: name.clone(),
            description: String::new(),
            author: String::new(),
            icon: None,
            language: String::from("und"),
            languages: Vec::new(),
            engine_type,
            capabilities: EngineCapabilities::empty(),
            backend_preference: match engine_type {
                EngineType::Keyboard => BackendPreference::FfiPreferred,
                EngineType::Voice => BackendPreference::ProcessPreferred,
            },
            name,
        }
    }

    /// The languages the engine declares: `languages` when non-empty,
    /// otherwise the single primary `language`.
    pub fn effective_languages(&self) -> &[String] {
        if self.languages.is_empty() {
            std::slice::from_ref(&self.language)
        } else {
            &self.languages
        }
    }
}

/// Opaque handle to the Typio instance, passed to `Engine::init`.
///
/// Engines must not store this handle beyond the scope of the call.
pub struct InstanceHandle;

impl InstanceHandle {
    pub(crate) fn new() -> Self {
        Self
    }
}

/// Opaque handle to an input context.
pub struct InputContext {
    raw: *mut typio_abi::TypioInputContext,
}

impl InputContext {
    pub(crate) fn from_raw(raw: *mut crate::TypioInputContext) -> Self {
        Self { raw: raw.cast() }
    }

    pub(crate) fn as_raw(&self) -> *mut typio_abi::TypioInputContext {
        self.raw
    }
}

/// An invocable command exposed by an engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Machine-readable command identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
}

/// Base trait implemented by every engine.
///
/// All methods operate on safe Rust types. Implementors are ordinary Rust
/// structs; there is no `extern "C"` boilerplate.
pub trait Engine: Send {
    /// Return static engine metadata.
    fn info(&self) -> &EngineInfo;

    /// Initialize the engine with the given instance handle.
    fn init(&mut self, instance: &mut InstanceHandle) -> Result<()>;

    /// Deactivate the engine (e.g. on engine switch).
    fn deactivate(&mut self);

    /// Notify that an input context has received focus.
    fn focus_in(&mut self, ctx: &mut InputContext);

    /// Notify that an input context has lost focus.
    fn focus_out(&mut self, ctx: &mut InputContext);

    /// Reset engine state for the given context.
    fn reset(&mut self, ctx: &mut InputContext);

    /// Reload engine-specific configuration.
    fn reload_config(&mut self) -> Result<()>;

    /// Down-cast to keyboard-specific operations, if applicable.
    fn as_keyboard(&mut self) -> Option<&mut dyn KeyboardEngine> {
        None
    }

    /// Immutable down-cast to keyboard-specific operations.
    fn as_keyboard_ref(&self) -> Option<&dyn KeyboardEngine> {
        None
    }

    /// Down-cast to voice-specific operations, if applicable.
    fn as_voice(&mut self) -> Option<&mut dyn VoiceEngine> {
        None
    }

    /// Immutable down-cast to voice-specific operations.
    fn as_voice_ref(&self) -> Option<&dyn VoiceEngine> {
        None
    }

    /* ----------------------------------------------------------------- */
    /* Engine command surface (ADR-0008)                                 */
    /*                                                                   */
    /* Engine-owned *properties* are unified with the config schema      */
    /* layer; engines declare schemas via `typio_config_schema_register_*`
    and react to value changes via `on_config_change` (below) or       */
    /* via the existing `reload_config` callback for full reloads.       */
    /* ----------------------------------------------------------------- */

    /// List commands exposed by this engine.
    fn list_commands(&self) -> Vec<Command> {
        vec![]
    }

    /// Invoke a command by id.
    fn invoke_command(&mut self, _id: &str) -> Result<()> {
        Err(EngineError::NotSupported)
    }

    /// A config key the engine owns changed (ADR-0008).
    ///
    /// Default no-op; engines override when they need a live side effect
    /// (e.g. librime re-selecting a schema). The host calls this *after*
    /// the value is committed to the unified config tree.
    fn on_config_change(&mut self, _key: &str, _value: &str) {}

    /// Return whether the engine can process input right now.
    ///
    /// Engines with no asynchronous warm-up use the default `Ready` state.
    fn availability(&self) -> EngineAvailability {
        EngineAvailability::Ready
    }
}

/// Extension trait for keyboard engines.
pub trait KeyboardEngine: Engine {
    /// Process a key event within the given input context.
    fn process_key(&mut self, ctx: &mut InputContext, event: &KeyEvent) -> KeyProcessResult;

    /// Return all modes the engine supports.
    fn list_modes(&self) -> Vec<EngineMode> {
        Vec::new()
    }

    /// Return the currently active mode, if any.
    fn get_active_mode(&self, _ctx: &InputContext) -> Option<EngineMode> {
        None
    }

    /// Set the active mode by id. Passing `None` cycles to the next mode.
    fn set_active_mode(&mut self, _ctx: &mut InputContext, _mode_id: Option<&str>) -> Result<()> {
        Err(EngineError::NotSupported)
    }

    /// Commit the candidate at the given index.
    fn commit_candidate(&mut self, _ctx: &mut InputContext, _candidate_index: i32) -> Result<()> {
        Err(EngineError::NotSupported)
    }

    /// Drain a pending active-mode change observed since the last call.
    ///
    /// Out-of-process engines report their current active mode in the reply
    /// to any request that can change it (notably `process-key`). The backend
    /// caches that value and surfaces it here once, when it differs from the
    /// previously observed mode. The framework uses this to keep the host's
    /// indicator and tray in sync without polling. Returns `None` when nothing
    /// changed.
    fn take_changed_mode(&mut self) -> Option<EngineMode> {
        None
    }
}

/// Extension trait for voice engines.
///
/// Requires `Send + Sync` because `process_audio` may be called from an
/// inference thread while the main thread drives `focus_in` / `focus_out`.
pub trait VoiceEngine: Engine + Send + Sync {
    /// Process a chunk of audio samples and return transcribed text, if any.
    fn process_audio(&self, samples: &[f32]) -> Option<String>;
}
