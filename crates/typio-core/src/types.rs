//! C-compatible type definitions matching `include/typio/*.h`
//!
//! Shared ABI types are re-exported from `typio_abi`. Types that are
//! libtypio-specific (host-only configuration helpers and instance config) are
//! defined here.

use std::ffi::c_char;

/* Re-export everything from typio_abi except the opaque handles that
libtypio implements internally (TypioInputContext, TypioInstance,
TypioRegistry).  TypioVoiceSession is pure-opaque everywhere, so it
is re-exported. */

pub use typio_abi::{
    TYPIO_KEY_Alt_L, TYPIO_KEY_Alt_R, TYPIO_KEY_BackSpace, TYPIO_KEY_Control_L,
    TYPIO_KEY_Control_R, TYPIO_KEY_Delete, TYPIO_KEY_Down, TYPIO_KEY_End, TYPIO_KEY_Escape,
    TYPIO_KEY_F1, TYPIO_KEY_F2, TYPIO_KEY_F3, TYPIO_KEY_F4, TYPIO_KEY_F5, TYPIO_KEY_F6,
    TYPIO_KEY_F7, TYPIO_KEY_F8, TYPIO_KEY_F9, TYPIO_KEY_F10, TYPIO_KEY_F11, TYPIO_KEY_F12,
    TYPIO_KEY_Home, TYPIO_KEY_KP_Enter, TYPIO_KEY_Left, TYPIO_KEY_Page_Down, TYPIO_KEY_Page_Up,
    TYPIO_KEY_Return, TYPIO_KEY_Right, TYPIO_KEY_Shift_L, TYPIO_KEY_Shift_R, TYPIO_KEY_Super_L,
    TYPIO_KEY_Super_R, TYPIO_KEY_Tab, TYPIO_KEY_Up, TYPIO_KEY_space, TypioAbiVersion,
    TypioCandidate, TypioCommitCallback, TypioComposition, TypioCompositionCallback,
    TypioConfigField, TypioConfigType, TypioDeleteSurroundingCallback, TypioEngine,
    TypioEngineAbiVersionFunc, TypioEngineAvailability, TypioEngineAvailabilityChangedCallback,
    TypioEngineBaseOps, TypioEngineChangedCallback, TypioEngineCommand, TypioEngineInfo,
    TypioEngineInfoFunc, TypioEngineSurfaceOps, TypioEngineType, TypioEvent, TypioEventData,
    TypioEventType, TypioFieldDefault, TypioFieldType, TypioKeyEvent, TypioKeyProcessResult,
    TypioKeyboardEngine, TypioKeyboardEngineFactory, TypioKeyboardEngineMode,
    TypioKeyboardEngineOps, TypioKeyboardModeChangedCallback, TypioLanguagesChangedCallback,
    TypioLogCallback, TypioLogEvent, TypioLogLevel, TypioModifier, TypioPreedit,
    TypioPreeditFormat, TypioPreeditSegment, TypioResult, TypioStatusIconChangedCallback,
    TypioStatusSalience, TypioVoiceEngine, TypioVoiceEngineChangedCallback,
    TypioVoiceEngineFactory, TypioVoiceEngineOps, TypioVoiceEvent, TypioVoiceSession,
};

/* -------------------------------------------------------------------------- */
/* Opaque handles implemented in other modules                                */
/* -------------------------------------------------------------------------- */

// TypioInputContext  → defined in src/input_context.rs
// TypioInstance      → defined in src/instance.rs
// TypioRegistry      → defined in src/c_api/registry.rs

/* -------------------------------------------------------------------------- */
/* Instance config                                                            */
/* -------------------------------------------------------------------------- */

/// Configuration passed to `typio_instance_init`.
#[repr(C)]
pub struct TypioInstanceConfig {
    /// Directory containing `core.toml` and engine configs.
    pub config_dir: *const c_char,
    /// Directory for runtime data (dictionaries, models).
    pub data_dir: *const c_char,
    /// Directory for transient state (active engine, user prefs).
    pub state_dir: *const c_char,
}
