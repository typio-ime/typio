//! Pending Wayland request tracking.
//!
//! Three protocol request/response pairs the host depends on are tracked here
//! so that a compositor which never answers is *named* in the logs — the
//! diagnostic that separates "our bug" from "the compositor did not send X":
//!
//! - `commit(serial)`  →  `done` event (the input-method serial handshake)
//! - `grab_keyboard`   →  `keymap` event (grab keymap delivery)
//! - anchor probe      →  `text_input_rectangle` event (caret-rect delivery)
//!
//! The design mirrors [`crate::panel_present_gate`]: an `Option<Instant>`
//! recorded when the request is sent, a `Duration` threshold, and one warn per
//! stalled episode (the compositor is never retried — a missing response is
//! almost always its bug, and retrying would only mask it). The earliest
//! in-flight deadline feeds the event-loop poll reducer so the warn fires
//! promptly instead of at the next unrelated wake-up.
//!
//! Beyond timeout detection, the two low-frequency pairs (`grab`→`keymap`,
//! probe→`rect`) also report the *measured* response latency at `debug` level
//! on success. This is the data that calibrates the thresholds above — if a
//! healthy compositor answers in ~50 ms, the multi-second timeouts are clearly
//! generous; if it takes ~800 ms, the timeouts may need tightening. The
//! high-frequency `commit`→`done` pair measures but does not log, to avoid
//! flooding (every candidate update commits).
//!
//! These are *diagnostic* thresholds, not correctness gates. Wayland carries
//! no real-time guarantee, so the values are deliberately generous and only a
//! missing response that crosses them is noteworthy.

use std::time::{Duration, Instant};

/// No `done` after this long means the compositor is not replying to the
/// input-method serial handshake. 2 s is well beyond any healthy compositor,
/// which acks within a frame (~16 ms).
pub const COMMIT_DONE_TIMEOUT: Duration = Duration::from_millis(2000);

/// No `keymap` after this long means the grab was created but the compositor
/// never delivered its keymap, so the IM will receive no keys. 3 s covers the
/// slowest observed keymap load (fd read + XKB compile) with margin.
pub const GRAB_KEYMAP_TIMEOUT: Duration = Duration::from_millis(3000);

/// No `text_input_rectangle` after an anchor probe means the compositor is not
/// reporting the caret rect for this popup. 1.5 s mirrors the coordinator's
/// anchor-probe ceiling; exceeding it is the same condition the coordinator
/// already falls back from — this only makes it visible.
pub const PROBE_RECT_TIMEOUT: Duration = Duration::from_millis(1500);

/// A single in-flight request awaiting a compositor response.
#[derive(Default)]
struct PendingSlot {
    /// When the request was sent; `None` when nothing is pending.
    sent_at: Option<Instant>,
    /// Suppresses repeats within one episode (request sent → response).
    reported: bool,
}

impl PendingSlot {
    /// Record that a request was just sent.
    fn note_sent(&mut self, now: Instant) {
        self.sent_at = Some(now);
        self.reported = false;
    }

    /// Record that the awaited response arrived. Returns the elapsed time
    /// since the request was sent, if one was outstanding.
    fn note_resolved(&mut self, now: Instant) -> Option<Duration> {
        let elapsed = self.sent_at.map(|since| now.saturating_duration_since(since));
        self.sent_at = None;
        self.reported = false;
        elapsed
    }

    /// Milliseconds remaining until the timeout fires, for poll-timeout
    /// reduction. `None` when nothing is in flight.
    fn deadline_remaining_ms(&self, timeout: Duration, now: Instant) -> Option<i32> {
        let since = self.sent_at?;
        let elapsed = now.saturating_duration_since(since);
        if elapsed >= timeout {
            Some(0)
        } else {
            Some((timeout - elapsed).as_millis().min(i32::MAX as u128) as i32)
        }
    }

    /// Returns `true` exactly once per stalled episode, when the request has
    /// timed out without a response.
    fn check_timed_out(&mut self, timeout: Duration, now: Instant) -> bool {
        if let Some(since) = self.sent_at {
            if !self.reported && now.saturating_duration_since(since) >= timeout {
                self.reported = true;
                return true;
            }
        }
        false
    }
}

/// Tracks the three protocol request/response pairs the host relies on.
///
/// Owned by [`crate::input_method::InputMethodState`] and ticked once per
/// main-loop iteration.
#[derive(Default)]
pub struct PendingRequestTracker {
    commit_done: PendingSlot,
    grab_keymap: PendingSlot,
    probe_rect: PendingSlot,
}

impl PendingRequestTracker {
    /// A `commit(serial)` was just sent; expect a `done` event.
    pub fn note_commit_sent(&mut self, now: Instant) {
        self.commit_done.note_sent(now);
    }

    /// A `done` event arrived, resolving any outstanding commit. The measured
    /// latency is discarded: commits fire on every candidate update, so logging
    /// per response would flood. The slot is still measured so a stall remains
    /// detectable via [`PendingRequestTracker::check_timeouts`].
    pub fn note_done_received(&mut self, now: Instant) {
        self.commit_done.note_resolved(now);
    }

    /// `grab_keyboard` was just sent; expect a `keymap` event.
    pub fn note_grab_sent(&mut self, now: Instant) {
        self.grab_keymap.note_sent(now);
    }

    /// A `keymap` event arrived, resolving any outstanding grab. Emits a
    /// `debug` latency line — grabs are infrequent (one per focus-in), so the
    /// data is both useful and unobtrusive, and it calibrates
    /// [`GRAB_KEYMAP_TIMEOUT`].
    pub fn note_keymap_received(&mut self, now: Instant) {
        if let Some(elapsed) = self.grab_keymap.note_resolved(now) {
            tracing::debug!(
                target: "typio.wayland.grab",
                latency_ms = elapsed.as_secs_f64() * 1000.0,
                timeout_ms = GRAB_KEYMAP_TIMEOUT.as_millis() as u64,
                "grab keymap delivered"
            );
        }
    }

    /// An anchor probe was just sent; expect a `text_input_rectangle` event.
    pub fn note_probe_sent(&mut self, now: Instant) {
        self.probe_rect.note_sent(now);
    }

    /// A `text_input_rectangle` event arrived, resolving any outstanding probe.
    /// Emits a `debug` latency line — probes are infrequent, and the data
    /// calibrates [`PROBE_RECT_TIMEOUT`].
    pub fn note_rect_received(&mut self, now: Instant) {
        if let Some(elapsed) = self.probe_rect.note_resolved(now) {
            tracing::debug!(
                target: "typio.panel.host",
                latency_ms = elapsed.as_secs_f64() * 1000.0,
                timeout_ms = PROBE_RECT_TIMEOUT.as_millis() as u64,
                "anchor rectangle delivered"
            );
        }
    }

    /// Earliest in-flight deadline in ms, for the event-loop poll reducer.
    /// `None` when no request is pending.
    pub fn min_deadline_ms(&self, now: Instant) -> Option<i32> {
        [
            self.commit_done
                .deadline_remaining_ms(COMMIT_DONE_TIMEOUT, now),
            self.grab_keymap
                .deadline_remaining_ms(GRAB_KEYMAP_TIMEOUT, now),
            self.probe_rect
                .deadline_remaining_ms(PROBE_RECT_TIMEOUT, now),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Emit one warn per stalled episode. Call this every main-loop tick.
    pub fn check_timeouts(&mut self, now: Instant) {
        if self.commit_done.check_timed_out(COMMIT_DONE_TIMEOUT, now) {
            tracing::warn!(
                target: "typio.wayland.frontend",
                timeout_ms = COMMIT_DONE_TIMEOUT.as_millis() as u64,
                "commit(serial) sent but no done event within the timeout — \
                 the compositor is not replying to the input-method serial \
                 handshake; if this persists the compositor, not typio, is \
                 the likely cause"
            );
        }
        if self.grab_keymap.check_timed_out(GRAB_KEYMAP_TIMEOUT, now) {
            tracing::warn!(
                target: "typio.wayland.grab",
                timeout_ms = GRAB_KEYMAP_TIMEOUT.as_millis() as u64,
                "grab_keyboard sent but no keymap event within the timeout — \
                 the compositor did not deliver the grab keymap, so the IM \
                 will not receive keys until it does; suspect the compositor's \
                 input-method grab implementation"
            );
        }
        if self.probe_rect.check_timed_out(PROBE_RECT_TIMEOUT, now) {
            tracing::warn!(
                target: "typio.panel.host",
                timeout_ms = PROBE_RECT_TIMEOUT.as_millis() as u64,
                "anchor probe sent but no text_input_rectangle within the \
                 timeout — the compositor did not report the caret rect; the \
                 panel fell back to the default anchor; suspect the \
                 compositor's popup-surface handling"
            );
        }
    }
}
