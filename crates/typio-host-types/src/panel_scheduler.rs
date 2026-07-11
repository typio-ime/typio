//! Minimal dirty-state marker for candidate Panel presentation.
//!
//! Presentation policy belongs to the host's Panel driver. This shared type is
//! only the platform/host hand-off that records whether newer visual state
//! still needs to reach the compositor.

/// Whether candidate Panel work is pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelScheduleState {
    #[default]
    Idle,
    Dirty,
}

impl PanelScheduleState {
    pub fn is_dirty(self) -> bool {
        self == Self::Dirty
    }

    pub fn mark_dirty(&mut self) {
        *self = Self::Dirty;
    }

    pub fn complete(&mut self) {
        *self = Self::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_state_transitions_are_explicit_and_idempotent() {
        let mut state = PanelScheduleState::default();
        assert!(!state.is_dirty());

        state.mark_dirty();
        state.mark_dirty();
        assert!(state.is_dirty());

        state.complete();
        state.complete();
        assert!(!state.is_dirty());
    }
}
