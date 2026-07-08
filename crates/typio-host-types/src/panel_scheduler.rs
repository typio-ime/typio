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

/// Mark the panel schedule as needing a redraw.
pub fn mark_dirty() -> PanelScheduleState {
    PanelScheduleState::Dirty
}

/// Mark the panel schedule as settled (no redraw pending).
pub fn complete() -> PanelScheduleState {
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

/// `typio_wl_panel_scheduler_should_settle` — the Idle-escape valve.
///
/// A `Dirty` schedule only ever leaves `Dirty` via the flush path
/// ([`should_flush`] → render → `complete()`). But the flush path is
/// *gated* on focus: when there is nothing to show or no focus to show
/// it under, `should_flush` returns `false` and the state is never
/// revisited, so a stray `mark_dirty` pins the schedule `Dirty`
/// indefinitely and the tick logs spin every loop iteration.
///
/// `should_settle` names the exact condition under which a `Dirty`
/// schedule is *provably unable* to flush now and *cannot become* flushable
/// by waiting — either there are no candidates to present, or the focus
/// context is gone. In both cases holding `Dirty` is pure waste, so the
/// caller moves to [`complete()`] (Idle). When candidates *and* a focus
/// edge both exist, only a transient throttle (anchor not ready, frame
/// soft-gate) can hold the flush back, and that *can* resolve on its own —
/// so the schedule must stay `Dirty` and `should_settle` returns `false`.
pub fn should_settle(
    state: PanelScheduleState,
    candidate_count: usize,
    has_context: bool,
    has_session: bool,
) -> bool {
    state == PanelScheduleState::Dirty && (candidate_count == 0 || !has_context || !has_session)
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_dirty_queues_redraw() {
        assert_eq!(mark_dirty(), PanelScheduleState::Dirty);
    }

    #[test]
    fn complete_returns_idle() {
        assert_eq!(complete(), PanelScheduleState::Idle);
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

    #[test]
    fn should_settle_escapes_dirty_when_unflushable() {
        // No candidates to present: nothing to do, settle to Idle.
        assert!(should_settle(PanelScheduleState::Dirty, 0, true, true));
        // Context gone or session gone: no focus to present under.
        assert!(should_settle(PanelScheduleState::Dirty, 3, false, true));
        assert!(should_settle(PanelScheduleState::Dirty, 3, true, false));

        // Candidates present AND focus present: a transient throttle
        // (anchor / frame gate) could still clear, so hold Dirty.
        assert!(!should_settle(PanelScheduleState::Dirty, 3, true, true));
        // Idle is never "settling" — it is the terminal state already.
        assert!(!should_settle(PanelScheduleState::Idle, 0, true, true));
        assert!(!should_settle(PanelScheduleState::Idle, 3, false, false));
    }
}
