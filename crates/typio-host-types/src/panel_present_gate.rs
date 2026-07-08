//! Tracks which candidate-panel snapshot has already been presented.
//!
//! The panel renders offscreen on the CPU and attaches host-owned SHM buffers
//! via `wl_surface_attach_commit`, which is non-blocking. Back-pressure is
//! handled by the SHM buffer pool (drops frames when all buffers are busy), not
//! by frame-callback pacing — so candidate updates are never delayed waiting
//! for `wl_surface.frame done`. This record provides deduplication so
//! unrelated wakeups (focus churn, indicator timer) don't cause redundant
//! re-paints of the same composition.

/// Tracks which candidate-panel snapshot has already reached the compositor.
///
/// State changes are coalesced freely; rendering consumes only the newest state.
/// `invalidate` is called after non-composition changes (scale, hide/show,
/// theme reload, popup owner switch) that require a repaint even though the
/// composition sequence number hasn't changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresentationRecord {
    generation: u64,
    presented: Option<(u64, u64)>,
}

impl PresentationRecord {
    /// Invalidate the last-presented marker after a non-composition change
    /// that still requires a redraw.
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

#[cfg(test)]
mod tests {
    use super::*;

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
