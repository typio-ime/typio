//! Pure scheduling policy for the candidate panel's dirty tick.
//!
//! This is the pure decision core of the panel redraw scheduler — given the
//! current schedule state and a few live flags, it decides whether the panel
//! should flush this tick. No I/O, no Wayland handles.

/// Per-tick schedule state of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelScheduleState {
    /// Nothing pending.
    #[default]
    Idle = 0,
    /// A redraw is queued for the next flush opportunity.
    Dirty = 1,
}

/// `typio_wl_panel_scheduler_mark_dirty`.
pub fn mark_dirty(_current: PanelScheduleState) -> PanelScheduleState {
    PanelScheduleState::Dirty
}

/// `typio_wl_panel_scheduler_complete`.
pub fn complete() -> PanelScheduleState {
    PanelScheduleState::Idle
}

/// `typio_wl_panel_scheduler_cancel`.
pub fn cancel() -> PanelScheduleState {
    PanelScheduleState::Idle
}

/// `typio_wl_panel_scheduler_should_flush`.
pub fn should_flush(
    state: PanelScheduleState,
    has_session: bool,
    has_context: bool,
    context_focused: bool,
) -> bool {
    state != PanelScheduleState::Idle && has_session && has_context && context_focused
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_dirty_queues_redraw() {
        assert_eq!(
            mark_dirty(PanelScheduleState::Idle),
            PanelScheduleState::Dirty
        );
        assert_eq!(
            mark_dirty(PanelScheduleState::Dirty),
            PanelScheduleState::Dirty
        );
    }

    #[test]
    fn complete_returns_idle() {
        assert_eq!(complete(), PanelScheduleState::Idle);
    }

    #[test]
    fn cancel_returns_idle() {
        assert_eq!(cancel(), PanelScheduleState::Idle);
    }

    #[test]
    fn should_flush_requires_all_conditions() {
        // Idle state never flushes even with everything present.
        assert!(!should_flush(PanelScheduleState::Idle, true, true, true));
        // Dirty + everything present → flush.
        assert!(should_flush(PanelScheduleState::Dirty, true, true, true));
        // Missing any flag suppresses the flush.
        assert!(!should_flush(PanelScheduleState::Dirty, false, true, true));
        assert!(!should_flush(PanelScheduleState::Dirty, true, false, true));
        assert!(!should_flush(PanelScheduleState::Dirty, true, true, false));
    }
}
