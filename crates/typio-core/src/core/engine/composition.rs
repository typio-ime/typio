//! Owned output values produced by engine processes.

/// Rendering hint for one preedit segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreeditFormat {
    /// No decoration.
    #[default]
    None,
    /// Underline decoration.
    Underline,
    /// Selected/highlighted decoration.
    Highlight,
}

/// Owned preedit segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreeditSegment {
    /// Segment text.
    pub text: String,
    /// Rendering hint.
    pub format: PreeditFormat,
}

/// Owned candidate entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Candidate text.
    pub text: String,
    /// Optional annotation.
    pub comment: Option<String>,
    /// Optional selection label.
    pub label: Option<String>,
}

/// Atomic preedit and candidate snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Composition {
    /// Preedit segments.
    pub segments: Vec<PreeditSegment>,
    /// UTF-8 cursor position.
    pub cursor_pos: i32,
    /// Candidate entries.
    pub candidates: Vec<Candidate>,
    /// Zero-based page number.
    pub page: i32,
    /// Candidate page size.
    pub page_size: i32,
    /// Total candidate count.
    pub total: i32,
    /// Selected candidate index.
    pub selected: i32,
    /// Previous-page availability.
    pub has_prev: bool,
    /// Next-page availability.
    pub has_next: bool,
    /// Host-managed selection flags.
    pub host_managed_selection: u32,
}

/// Ordered output emitted while handling one engine request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextOutput {
    /// Commit text and clear the in-flight composition.
    Commit(String),
    /// Replace the in-flight composition atomically.
    Composition(Composition),
    /// Clear the in-flight composition.
    Clear,
}
