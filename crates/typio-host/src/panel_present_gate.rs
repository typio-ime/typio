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
/// This stays below the old 200 ms recovery path, but above one 30 Hz frame.
/// A healthy compositor normally wakes earlier through `wl_surface.frame`;
/// a compositor that drops callbacks timer-paces at a conservative cadence
/// instead of filling the swapchain and blocking in present.
pub const PANEL_FRAME_CALLBACK_SOFT_LIMIT: Duration = Duration::from_millis(50);

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
}
