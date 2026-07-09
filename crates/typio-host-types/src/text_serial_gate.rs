//! Serial-aware staging for compositor-facing text transactions.
//!
//! `zwp_input_method_v2` applies `set_preedit_string` / `commit_string` on
//! `commit(serial)`. The serial equals the number of compositor `done` events
//! already received. Submitting multiple *preedit-only* updates with the same
//! serial (across event-loop ticks) can leave the later preedit stale while the
//! engine state is already correct (fast Rime `n` then `i`).
//!
//! Rules:
//!
//! 1. **Preedit-only** updates may be deferred when the current serial was
//!    already used; the latest preedit is flushed when `done` advances the
//!    serial (or when a later commit forces a send).
//! 2. **`commit_string` always sends immediately.** Compositor `done` is not an
//!    ack of our text `commit` — it arrives on compositor-driven state changes.
//!    Waiting for `done` before sending commit text stalls Space/上屏 until an
//!    unrelated `done` (or another key) unblocks the gate.
//!
//! ADR-0042 still coalesces composition-only preedit within one key-batch
//! drain in the keyboard router; this gate covers the cross-tick preedit case
//! without holding real commit text hostage.

/// A text transaction waiting for a free input-method serial.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeferredTextTransaction {
    /// Latest preedit text + cursor, if any.
    pub preedit: Option<(String, u32)>,
}

impl DeferredTextTransaction {
    /// Merge a preedit-only update (latest wins).
    pub fn merge_preedit(&mut self, preedit: Option<(&str, u32)>) {
        if let Some((text, cursor)) = preedit {
            self.preedit = Some((text.to_string(), cursor));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.preedit.is_none()
    }
}

/// Decision from [`TextSerialGate::submit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSubmit {
    /// Send this payload now with the current serial.
    Send {
        commit_text: Option<String>,
        preedit: Option<(String, u32)>,
    },
    /// Serial already used; pure preedit was staged for the next `done`.
    Deferred,
}

/// Tracks whether the current serial has already carried a text commit and
/// holds preedit-only updates until `done` or a forced commit-string send.
#[derive(Debug, Default)]
pub struct TextSerialGate {
    /// Serial for which a text `commit` was already sent, if any.
    committed_serial: Option<u32>,
    deferred: Option<DeferredTextTransaction>,
}

impl TextSerialGate {
    /// Submit a text transaction for `serial`.
    ///
    /// Returns [`TextSubmit::Send`] when the wire commit may proceed, or
    /// [`TextSubmit::Deferred`] when a pure preedit was staged.
    pub fn submit(
        &mut self,
        serial: u32,
        commit_text: Option<&str>,
        preedit: Option<(&str, u32)>,
    ) -> TextSubmit {
        if commit_text.is_none() && preedit.is_none() {
            return TextSubmit::Deferred;
        }

        // Real commit text must never wait on compositor `done`. Force send
        // immediately. Bare commit drops any deferred preedit (protocol:
        // commit replaces the current preedit); commit+replacement-preedit
        // uses the caller's preedit.
        if commit_text.is_some() {
            let _ = self.deferred.take();
            let pending_preedit = preedit.map(|(t, c)| (t.to_string(), c));
            self.committed_serial = Some(serial);
            return TextSubmit::Send {
                commit_text: commit_text.map(str::to_string),
                preedit: pending_preedit,
            };
        }

        // Preedit-only.
        if self.committed_serial == Some(serial) {
            self.deferred
                .get_or_insert_with(DeferredTextTransaction::default)
                .merge_preedit(preedit);
            return TextSubmit::Deferred;
        }

        // Gate open: fold leftover deferred preedit (rare) then send.
        if let Some(d) = self.deferred.take() {
            let mut pending = d.preedit;
            if let Some((text, cursor)) = preedit {
                pending = Some((text.to_string(), cursor));
            }
            self.committed_serial = Some(serial);
            return TextSubmit::Send {
                commit_text: None,
                preedit: pending,
            };
        }

        self.committed_serial = Some(serial);
        TextSubmit::Send {
            commit_text: None,
            preedit: preedit.map(|(t, c)| (t.to_string(), c)),
        }
    }

    /// Called after the compositor `done` advances the serial.
    ///
    /// Returns a deferred preedit-only transaction to send with `new_serial`.
    pub fn on_serial_advanced(&mut self, new_serial: u32) -> Option<DeferredTextTransaction> {
        self.committed_serial = None;
        let d = self.deferred.take()?;
        if d.is_empty() {
            return None;
        }
        self.committed_serial = Some(new_serial);
        Some(d)
    }

    /// Drop gate state (focus lost / hard reset).
    pub fn clear(&mut self) {
        self.committed_serial = None;
        self.deferred = None;
    }

    #[cfg(test)]
    pub fn is_blocked_for(&self, serial: u32) -> bool {
        self.committed_serial == Some(serial)
    }

    #[cfg(test)]
    pub fn has_deferred(&self) -> bool {
        self.deferred.as_ref().is_some_and(|d| !d.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_submit_sends_immediately() {
        let mut gate = TextSerialGate::default();
        match gate.submit(1, None, Some(("n", 1))) {
            TextSubmit::Send {
                commit_text: None,
                preedit: Some((t, c)),
            } => {
                assert_eq!(t, "n");
                assert_eq!(c, 1);
            }
            other => panic!("expected Send, got {other:?}"),
        }
        assert!(gate.is_blocked_for(1));
    }

    #[test]
    fn second_preedit_same_serial_defers_and_coalesces() {
        let mut gate = TextSerialGate::default();
        assert!(matches!(
            gate.submit(1, None, Some(("n", 1))),
            TextSubmit::Send { .. }
        ));
        assert_eq!(
            gate.submit(1, None, Some(("ni", 2))),
            TextSubmit::Deferred
        );
        assert!(gate.has_deferred());

        let d = gate.on_serial_advanced(2).expect("deferred preedit");
        assert_eq!(d.preedit, Some(("ni".to_string(), 2)));
        assert!(gate.is_blocked_for(2));
    }

    #[test]
    fn commit_string_forces_send_even_when_serial_blocked() {
        let mut gate = TextSerialGate::default();
        let _ = gate.submit(3, None, Some(("ni", 2)));
        // Later pure preedit deferred.
        assert_eq!(
            gate.submit(3, None, Some(("nih", 3))),
            TextSubmit::Deferred
        );

        // Space 上屏 must not wait for done.
        match gate.submit(3, Some("你"), None) {
            TextSubmit::Send {
                commit_text: Some(c),
                preedit: None,
            } => assert_eq!(c, "你"),
            other => panic!("expected forced Send of commit, got {other:?}"),
        }
        assert!(!gate.has_deferred());
    }

    #[test]
    fn commit_with_replacement_preedit_keeps_preedit() {
        let mut gate = TextSerialGate::default();
        let _ = gate.submit(1, None, Some(("nihao", 5)));
        match gate.submit(1, Some("你"), Some(("hao", 3))) {
            TextSubmit::Send {
                commit_text: Some(c),
                preedit: Some((p, cur)),
            } => {
                assert_eq!(c, "你");
                assert_eq!(p, "hao");
                assert_eq!(cur, 3);
            }
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn forced_commit_drops_stale_deferred_preedit() {
        let mut gate = TextSerialGate::default();
        let _ = gate.submit(1, None, Some(("n", 1)));
        let _ = gate.submit(1, None, Some(("ni", 2))); // deferred
        match gate.submit(1, Some("你"), None) {
            TextSubmit::Send {
                commit_text: Some(c),
                preedit: None,
            } => assert_eq!(c, "你"),
            other => panic!("expected bare commit without deferred preedit, got {other:?}"),
        }
    }

    #[test]
    fn done_with_no_deferred_opens_gate() {
        let mut gate = TextSerialGate::default();
        let _ = gate.submit(1, None, Some(("n", 1)));
        assert!(gate.on_serial_advanced(2).is_none());
        assert!(!gate.is_blocked_for(2));
        assert!(matches!(
            gate.submit(2, None, Some(("ni", 2))),
            TextSubmit::Send { .. }
        ));
    }

    #[test]
    fn clear_drops_deferred() {
        let mut gate = TextSerialGate::default();
        let _ = gate.submit(1, None, Some(("n", 1)));
        let _ = gate.submit(1, None, Some(("ni", 2)));
        gate.clear();
        assert!(!gate.has_deferred());
        assert!(!gate.is_blocked_for(1));
    }
}
