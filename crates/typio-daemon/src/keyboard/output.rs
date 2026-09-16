//! Owned engine output staging for the Wayland router.

use crate::candidate_guard::HostSelectionFlags;
use typio_runtime::input_context::ContextEvent;

/// Engine output staged until the router updates Wayland state.
#[derive(Default)]
pub(super) struct PendingEngineOutput {
    pub commit: Option<String>,
    pub composition: Option<PendingComposition>,
}

impl PendingEngineOutput {
    pub fn absorb(&mut self, event: ContextEvent) {
        match event {
            ContextEvent::Commit(text) => self.commit = Some(text),
            ContextEvent::Composition(composition) => {
                self.composition = Some(PendingComposition {
                    preedit_text: composition
                        .segments
                        .iter()
                        .map(|segment| segment.text.as_str())
                        .collect(),
                    cursor_pos: composition.cursor_pos,
                    candidates: composition
                        .candidates
                        .into_iter()
                        .map(|candidate| candidate.text)
                        .collect(),
                    selected: composition.selected.max(0) as usize,
                    has_prev: composition.has_prev,
                    has_next: composition.has_next,
                    host_managed_selection: HostSelectionFlags::from_bits_truncate(
                        composition.host_managed_selection,
                    ),
                });
            }
            ContextEvent::DeleteSurrounding { .. } => {
                // The Wayland delete-surrounding path is not wired yet. Keeping
                // the event owned and typed prevents it from becoming an FFI
                // callback again when support is added.
            }
        }
    }
}

/// Pending composition state since the last key dispatch.
#[derive(Default)]
pub(super) struct PendingComposition {
    pub preedit_text: String,
    pub cursor_pos: i32,
    pub candidates: Vec<String>,
    pub selected: usize,
    pub has_prev: bool,
    pub has_next: bool,
    pub host_managed_selection: HostSelectionFlags,
}
