/// Voice session state machine states.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEvent {
    /// State transitioned.
    StateChange(VoiceState),
    /// Transcription result available.
    Result(String),
    /// An error occurred.
    Error(String),
}
