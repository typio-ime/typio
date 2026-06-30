//! Input events for keyboard engines.

/// Keysym values mirror XKB / Linux input-event-codes where applicable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySym {
    /// ASCII alphanumeric pass-through (e.g. 'a', 'Z', '0').
    Ascii(u8),
    /// Left Shift modifier.
    ShiftL,
    /// Right Shift modifier.
    ShiftR,
    /// Left Control modifier.
    ControlL,
    /// Right Control modifier.
    ControlR,
    /// Left Alt modifier.
    AltL,
    /// Right Alt modifier.
    AltR,
    /// Left Meta / Super modifier.
    MetaL,
    /// Right Meta / Super modifier.
    MetaR,
    /// Return / Enter key.
    Return,
    /// Escape key.
    Escape,
    /// BackSpace key.
    BackSpace,
    /// Tab key.
    Tab,
    /// Left arrow key.
    Left,
    /// Right arrow key.
    Right,
    /// Up arrow key.
    Up,
    /// Down arrow key.
    Down,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up key.
    PageUp,
    /// Page Down key.
    PageDown,
    /// Function key (F1–F35).
    F(u8),
    /// Unlisted key with a raw hardware code.
    Raw(u32),
}

/// Key press or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyState {
    /// Key was pressed.
    Press,
    /// Key was released.
    Release,
}

/// A single key event delivered to a keyboard engine.
///
/// This is the lossless internal mirror of the public `TypioKeyEvent`
/// (`include/typio/abi/event.h`): every field the host fills in must round-trip
/// to the engine unchanged. Do not drop fields here — the host's resolved
/// `unicode` (dead keys, compose, level shifts) and `is_repeat` signal are not
/// recoverable from `sym`/`code` alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// Logical key symbol.
    pub sym: KeySym,
    /// Press or release.
    pub state: KeyState,
    /// Raw key code from the input device / compositor.
    pub code: u32,
    /// Modifier bitmask at the time of the event.
    pub modifiers: u32,
    /// Host-resolved Unicode codepoint, or 0 if not applicable.
    pub unicode: u32,
    /// Event timestamp in milliseconds.
    pub time: u64,
    /// Whether this event is a key-repeat.
    pub is_repeat: bool,
    /// Unshifted keysym (xkb level 0) for IME key binding matching.
    pub base_keysym: u32,
}

/// Result of processing a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyProcessResult {
    /// The engine consumed the key; do not pass to the application.
    Handled,
    /// The engine ignored the key; pass through to the application.
    NotHandled,
    /// The engine is still composing; pass the key to the application
    /// but keep the preedit alive.
    PassThrough,
}
