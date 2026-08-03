//! Engine mode and capability definitions.

/// Capabilities advertised by an engine.
///
/// Capabilities are negotiated by snake_case names in EngineHello. Required capabilities cause
/// the host to refuse loading if unsupported; optional capabilities are
/// best-effort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineCapabilities {
    /// Required capabilities (host must support these).
    pub required: Vec<String>,
    /// Optional capabilities (best-effort).
    pub optional: Vec<String>,
}

impl EngineCapabilities {
    /// Create an empty capability set.
    pub fn empty() -> Self {
        Self::default()
    }

    /// True when the engine declares `name` as required or optional.
    pub fn contains(&self, name: &str) -> bool {
        self.required.iter().any(|s| s == name) || self.optional.iter().any(|s| s == name)
    }
}

/// On-focus auto-reveal hint for a mode (see
/// `docs/dev/keyboard-status-salience.md`).
///
/// `salience` answers exactly one question: when the user *incidentally
/// focuses* a field already in this state, should the host auto-reveal it?
/// The default is silence — an engine that behaves like a plain keyboard
/// reports `Quiet` and produces no unprompted announcements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModeSalience {
    /// What you type is what you get (Latin/ascii/passthrough/off). Never
    /// auto-revealed on incidental focus.
    #[default]
    Quiet,
    /// Surprising to type into blind (composing native non-Latin script).
    /// Auto-revealed on incidental focus if the host environment agrees.
    Notable,
}

/// An engine mode (e.g., "Hiragana", "Katakana", "ASCII", "Browse").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EngineMode {
    /// Stable mode identifier (e.g. "native", "ascii", "browse").
    pub id: String,
    /// Human-readable label for UI display.
    pub label: String,
    /// Short indicator badge (e.g. "中", "A", "Browse").
    pub display_label: Option<String>,
    /// Optional icon name or path.
    pub icon: Option<String>,
    /// Engine-defined active profile (e.g. Rime schema id).
    pub profile_id: Option<String>,
    /// Human-readable profile name (e.g. "朙月拼音").
    pub profile_label: Option<String>,
    /// Optional detailed description.
    pub description: Option<String>,
    /// Whether this mode is currently active.
    pub is_active: bool,
    /// On-focus auto-reveal hint (host renders; engine declares meaning).
    pub salience: ModeSalience,
}
