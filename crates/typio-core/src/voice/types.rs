use std::ffi::c_char;

/// Voice session state machine states.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub enum VoiceState {
    /// Idle, no recording or inference in progress.
    Idle = 0,
    /// Model is being loaded.
    Loading = 1,
    /// Audio is being recorded.
    Recording = 2,
    /// Inference is running on captured audio.
    Processing = 3,
}

/// Internal voice event used for cross-thread communication.
pub enum VoiceEvent {
    /// State transitioned.
    StateChange(VoiceState),
    /// Transcription result available.
    Result(String),
    /// An error occurred.
    Error(&'static str),
}

/// C-compatible event structure delivered to the host callback.
#[repr(C)]
pub struct TypioVoiceSessionEvent {
    /// Event type discriminant.
    pub type_: TypioVoiceSessionEventType,
    /// Current session state.
    pub state: VoiceState,
    /// Transcribed text (owned by libtypio; caller must free via `typio_free_string`).
    pub text: *mut c_char,
    /// Error message (borrowed static string, never freed by caller).
    pub error: *const c_char,
}

/// Event type for voice session callbacks.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioVoiceSessionEventType {
    /// State changed.
    StateChange = 0,
    /// Transcription result.
    Result = 1,
    /// Error occurred.
    Error = 2,
}

/// Host-provided callback for voice session events.
pub type TypioVoiceSessionEventCallback =
    extern "C" fn(event: *const TypioVoiceSessionEvent, user_data: *mut std::ffi::c_void);
