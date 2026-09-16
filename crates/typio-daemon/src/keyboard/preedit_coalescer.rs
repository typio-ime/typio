//! Bounded coalescing for compositor-facing preedit updates.
//!
//! Keyboard events that are physically adjacent can arrive in separate
//! event-loop iterations. Sending each resulting preedit immediately can
//! produce two `zwp_input_method_v2.commit(serial)` requests with the same
//! serial while the compositor is advancing its input state. The engine and
//! candidate projection are already current, but the later inline preedit can
//! then remain visually stale.
//!
//! This coalescer gives pure preedit a short, bounded quiet period. The newest
//! value wins, real commit text still forces an immediate transaction, and the
//! hard deadline prevents a continuous input stream from starving display.

use std::time::{Duration, Instant};

/// Time without another preedit update before the latest value is ready.
pub(super) const PREEDIT_QUIET_PERIOD: Duration = Duration::from_millis(2);
/// Maximum time the first update in one burst may remain staged.
pub(super) const PREEDIT_MAX_DELAY: Duration = Duration::from_millis(4);

/// One complete preedit value ready for a Wayland text transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreeditUpdate {
    pub text: String,
    pub cursor: u32,
    pub engine_cursor_pos: i32,
}

/// Latest-wins preedit staging with a quiet deadline and a fixed hard limit.
#[derive(Debug, Default)]
pub(super) struct PreeditCoalescer {
    pending: Option<PreeditUpdate>,
    quiet_deadline: Option<Instant>,
    hard_deadline: Option<Instant>,
}

impl PreeditCoalescer {
    /// Stage a complete replacement value. A later update renews only the
    /// quiet deadline; the original hard deadline remains fixed.
    pub fn stage(&mut self, update: PreeditUpdate, now: Instant) {
        if self.pending.is_none() {
            self.hard_deadline = Some(now + PREEDIT_MAX_DELAY);
        }
        self.pending = Some(update);
        self.quiet_deadline = Some(now + PREEDIT_QUIET_PERIOD);
    }

    /// Latest staged value, used by the router's preedit deduplication.
    pub fn pending(&self) -> Option<&PreeditUpdate> {
        self.pending.as_ref()
    }

    /// Take the latest value regardless of its deadline. Real commit text uses
    /// this path so text ordering never waits for preedit coalescing.
    pub fn take(&mut self) -> Option<PreeditUpdate> {
        let pending = self.pending.take();
        self.quiet_deadline = None;
        self.hard_deadline = None;
        pending
    }

    /// Take the latest value once either bounded deadline has elapsed.
    pub fn take_if_due(&mut self, now: Instant) -> Option<PreeditUpdate> {
        if !self.is_due(now) {
            return None;
        }
        self.take()
    }

    /// Drop all staged state on focus/reset boundaries.
    pub fn clear(&mut self) {
        let _ = self.take();
    }

    /// Milliseconds until the next deadline, rounded up so an early `poll`
    /// wake cannot spin with a sub-millisecond remainder.
    pub fn deadline_remaining_ms(&self, now: Instant) -> Option<i32> {
        let deadline = self.next_deadline()?;
        if now >= deadline {
            return Some(0);
        }
        let remaining = deadline.duration_since(now);
        let millis = remaining.as_millis();
        let rounded_up = if remaining.subsec_nanos() % 1_000_000 == 0 {
            millis
        } else {
            millis.saturating_add(1)
        };
        Some(rounded_up.min(i32::MAX as u128) as i32)
    }

    fn is_due(&self, now: Instant) -> bool {
        self.pending.is_some() && self.next_deadline().is_some_and(|deadline| now >= deadline)
    }

    fn next_deadline(&self) -> Option<Instant> {
        match (self.quiet_deadline, self.hard_deadline) {
            (Some(quiet), Some(hard)) => Some(quiet.min(hard)),
            (Some(quiet), None) => Some(quiet),
            (None, Some(hard)) => Some(hard),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(text: &str) -> PreeditUpdate {
        PreeditUpdate {
            text: text.to_string(),
            cursor: text.len() as u32,
            engine_cursor_pos: -1,
        }
    }

    #[test]
    fn adjacent_cross_tick_updates_coalesce_to_latest() {
        let start = Instant::now();
        let mut c = PreeditCoalescer::default();
        c.stage(update("j"), start);
        c.stage(update("jr"), start + Duration::from_micros(500));

        assert!(c.take_if_due(start + Duration::from_millis(2)).is_none());
        assert_eq!(
            c.take_if_due(start + Duration::from_micros(2500)),
            Some(update("jr"))
        );
    }

    #[test]
    fn single_update_flushes_after_quiet_period() {
        let start = Instant::now();
        let mut c = PreeditCoalescer::default();
        c.stage(update("n"), start);

        assert!(
            c.take_if_due(start + PREEDIT_QUIET_PERIOD - Duration::from_nanos(1))
                .is_none()
        );
        assert_eq!(
            c.take_if_due(start + PREEDIT_QUIET_PERIOD),
            Some(update("n"))
        );
    }

    #[test]
    fn continuous_updates_cannot_extend_hard_deadline() {
        let start = Instant::now();
        let mut c = PreeditCoalescer::default();
        c.stage(update("a"), start);
        c.stage(update("ab"), start + Duration::from_millis(1));
        c.stage(update("abc"), start + Duration::from_millis(2));
        c.stage(update("abcd"), start + Duration::from_millis(3));

        assert!(
            c.take_if_due(start + PREEDIT_MAX_DELAY - Duration::from_nanos(1))
                .is_none()
        );
        assert_eq!(
            c.take_if_due(start + PREEDIT_MAX_DELAY),
            Some(update("abcd"))
        );
    }

    #[test]
    fn forced_take_never_waits_for_deadline() {
        let start = Instant::now();
        let mut c = PreeditCoalescer::default();
        c.stage(update("ni"), start);

        assert_eq!(c.take(), Some(update("ni")));
        assert_eq!(c.deadline_remaining_ms(start), None);
    }

    #[test]
    fn clear_drops_payload_and_deadlines() {
        let start = Instant::now();
        let mut c = PreeditCoalescer::default();
        c.stage(update("stale"), start);
        c.clear();

        assert!(c.pending().is_none());
        assert_eq!(c.deadline_remaining_ms(start), None);
    }

    #[test]
    fn deadline_remaining_rounds_positive_fraction_up() {
        let start = Instant::now();
        let mut c = PreeditCoalescer::default();
        c.stage(update("x"), start);

        assert_eq!(
            c.deadline_remaining_ms(start + Duration::from_micros(500)),
            Some(2)
        );
        assert_eq!(
            c.deadline_remaining_ms(start + PREEDIT_QUIET_PERIOD),
            Some(0)
        );
    }
}
