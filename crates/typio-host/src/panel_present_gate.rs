//! Present pacing policy for the candidate panel.
//!
//! `wl_surface.frame` callbacks are useful refresh hints, but input-method
//! popup surfaces can stop receiving them when the compositor occludes,
//! deprioritizes, or otherwise loses track of the popup. This module keeps the
//! callback in the pacing loop without letting a missing callback freeze
//! candidate updates.

use std::time::{Duration, Instant};

/// The longest a candidate update may wait for an outstanding frame callback
/// before the host presents the latest coalesced state anyway.
///
/// This stays below the old 200 ms recovery path and around a 50 Hz cadence.
/// A healthy compositor normally wakes earlier through `wl_surface.frame`;
/// a compositor that drops callbacks timer-paces at a conservative cadence
/// instead of filling the swapchain and blocking in present.
pub const PANEL_FRAME_CALLBACK_SOFT_LIMIT: Duration = Duration::from_millis(20);

/// Diagnostic threshold for an uninterrupted period with no frame callback.
///
/// Crossing this threshold no longer gates rendering. It only produces a
/// warning so compositor callback stalls remain visible in logs.
pub const PANEL_FRAME_CALLBACK_STALL_WARN: Duration = Duration::from_millis(200);

/// Whether the panel may present this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentDecision {
    /// Present now.
    Present,
    /// Wait until the deadline unless a frame callback arrives earlier.
    WaitUntil(Instant),
}

/// Tracks which candidate-panel snapshot has already reached `present`.
///
/// This is intentionally separate from the frame-callback gate. A frame
/// callback answers "should the compositor pace the next present?"; this record
/// answers "is there any newer panel content to present?". Keeping those
/// questions separate mirrors a double-buffered UI: state changes are
/// coalesced freely, and rendering consumes only the newest state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresentationRecord {
    generation: u64,
    presented: Option<(u64, u64)>,
}

impl PresentationRecord {
    /// Invalidate the last-presented marker after a non-composition change that
    /// still requires a redraw: scale change, hide/show transition, theme reload,
    /// or a different popup owner borrowing the surface.
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.presented = None;
    }

    /// Mark `composition_seq` as successfully submitted in the current
    /// generation.
    pub fn mark_presented(&mut self, composition_seq: u64) {
        self.presented = Some((self.generation, composition_seq));
    }

    /// True iff `composition_seq` was already submitted in the current
    /// generation.
    pub fn is_current(&self, composition_seq: u64) -> bool {
        self.presented == Some((self.generation, composition_seq))
    }
}

/// Decide whether an outstanding frame callback should still pace presents.
pub fn decide(pending_since: Option<Instant>, now: Instant) -> PresentDecision {
    let Some(pending_since) = pending_since else {
        return PresentDecision::Present;
    };
    let deadline = pending_since + PANEL_FRAME_CALLBACK_SOFT_LIMIT;
    if now >= deadline {
        PresentDecision::Present
    } else {
        PresentDecision::WaitUntil(deadline)
    }
}

/// Return the poll timeout needed to wake at `deadline`.
///
/// A deadline that has already passed returns `0`, so callers poll once and
/// immediately retry the panel flush without spinning.
pub fn deadline_remaining_ms(deadline: Instant, now: Instant) -> i32 {
    if now >= deadline {
        return 0;
    }
    let remaining = deadline.duration_since(now).as_millis();
    remaining.min(i32::MAX as u128) as i32
}

/// Whether an uninterrupted callback-missing episode is old enough to warn.
pub fn callback_stall_should_warn(missing_since: Option<Instant>, now: Instant) -> bool {
    missing_since
        .map(|since| now.saturating_duration_since(since) >= PANEL_FRAME_CALLBACK_STALL_WARN)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_callback_presents_immediately() {
        assert_eq!(decide(None, Instant::now()), PresentDecision::Present);
    }

    #[test]
    fn fresh_callback_waits_until_soft_limit() {
        let now = Instant::now();
        let pending = now - Duration::from_millis(1);
        assert_eq!(
            decide(Some(pending), now),
            PresentDecision::WaitUntil(pending + PANEL_FRAME_CALLBACK_SOFT_LIMIT)
        );
    }

    #[test]
    fn soft_limit_allows_present_without_callback() {
        let now = Instant::now();
        let pending = now - (PANEL_FRAME_CALLBACK_SOFT_LIMIT + Duration::from_millis(1));
        assert_eq!(decide(Some(pending), now), PresentDecision::Present);
    }

    #[test]
    fn deadline_remaining_ms_clamps_elapsed_deadline_to_zero() {
        let now = Instant::now();
        assert_eq!(
            deadline_remaining_ms(now - Duration::from_millis(1), now),
            0
        );
        assert_eq!(
            deadline_remaining_ms(now + Duration::from_millis(12), now),
            12
        );
    }

    #[test]
    fn stall_warning_uses_uninterrupted_missing_callback_age() {
        let now = Instant::now();
        assert!(!callback_stall_should_warn(None, now));
        assert!(!callback_stall_should_warn(Some(now), now));
        let stale = now - (PANEL_FRAME_CALLBACK_STALL_WARN + Duration::from_millis(1));
        assert!(callback_stall_should_warn(Some(stale), now));
    }

    #[test]
    fn presentation_record_dedupes_until_invalidated() {
        let mut record = PresentationRecord::default();
        assert!(!record.is_current(7));

        record.mark_presented(7);
        assert!(record.is_current(7));
        assert!(!record.is_current(8));

        record.invalidate();
        assert!(!record.is_current(7));

        record.mark_presented(7);
        assert!(record.is_current(7));
    }
}
