//! Candidate Panel presentation driver.
//!
//! This is the single policy-to-I/O boundary for candidate visibility, popup
//! ownership, presentation deduplication, and SHM back-pressure. The shared
//! platform state contributes only a dirty marker and the latest snapshot.

use crate::input_method::InputMethodFrontend;
use crate::keyboard::router::KeyboardRouter;
use crate::panel_coordinator::{FlushDecision, UiOwner};

/// Converge the latest candidate snapshot once after input and repeat work.
pub(super) fn flush_candidate_panel(frontend: &mut InputMethodFrontend, router: &KeyboardRouter) {
    let (dirty, selected, composition_seq, candidate_count) = {
        let state = frontend.state();
        (
            state.panel_schedule_state.is_dirty(),
            state.composition.selected_candidate,
            state.composition.composition_seq,
            state.composition.candidates.len(),
        )
    };
    if !dirty {
        return;
    }

    let focused = router.is_focused();
    let has_context = router.has_context();
    tracing::trace!(
        target: "typio.panel.scheduler",
        composition_seq,
        candidate_count,
        selected,
        focused,
        has_context,
        "panel driver step"
    );

    if !focused || !has_context {
        frontend.state_mut().panel_schedule_state.complete();
        tracing::debug!(
            target: "typio.panel.scheduler",
            composition_seq,
            candidate_count,
            focused,
            has_context,
            "panel: discard unpresentable dirty state"
        );
        return;
    }

    if candidate_count == 0 {
        hide_candidate_panel(frontend);
        return;
    }

    claim_candidate_panel(frontend);
    if frontend.state().panel_presentation_current(composition_seq) {
        frontend.state_mut().panel_schedule_state.complete();
        tracing::trace!(
            target: "typio.panel.host",
            composition_seq,
            "panel: skip present reason=already_presented"
        );
        return;
    }

    if frontend.panel_mut().is_none() {
        // Keep Dirty until platform initialization creates the surface.
        return;
    }

    let scale = frontend.state().buffer_scale;
    let candidates = frontend.state().composition.candidates.clone();
    let panel = frontend
        .panel_mut()
        .expect("candidate panel existence checked above");
    panel.set_scale(scale);
    panel.ensure_candidate_size(&candidates);
    if panel.draw_candidates(&candidates, selected, composition_seq) {
        let state = frontend.state_mut();
        state.mark_panel_presented(composition_seq);
        state.panel_schedule_state.complete();
    } else {
        // Keep Dirty. A wl_buffer release or another input event retries the
        // latest snapshot; no intermediate frame is authoritative.
        tracing::debug!(
            target: "typio.panel.host",
            composition_seq,
            "panel: present dropped — will retry latest state"
        );
    }
}

fn claim_candidate_panel(frontend: &mut InputMethodFrontend) {
    let state = frontend.state_mut();
    let owner_changed = {
        let coord = state.panel_coord_mut();
        let before = coord.visible_owner();
        if coord.anchor_ready() {
            coord.claim(UiOwner::Candidate);
        } else {
            // Candidate UI intentionally establishes fallback placement
            // immediately; unlike out-of-band status, it never waits here.
            let decision = coord.decide_positioned_flush(UiOwner::Candidate, "candidate");
            debug_assert_eq!(decision, FlushDecision::Show);
        }
        before != coord.visible_owner()
    };
    if owner_changed {
        state.invalidate_panel_presentation();
    }
}

fn hide_candidate_panel(frontend: &mut InputMethodFrontend) {
    let visible_owner = frontend.state().panel_coord().visible_owner();
    if visible_owner == UiOwner::Indicator || visible_owner == UiOwner::Voice {
        // An overlay owns the shared surface. The candidate path has no visual
        // work and must not detach another owner's buffer.
        frontend.state_mut().panel_schedule_state.complete();
        return;
    }

    {
        let state = frontend.state_mut();
        state.panel_coord_mut().hide(UiOwner::Candidate);
        state.invalidate_panel_presentation();
        state.panel_schedule_state.complete();
    }
    if let Some(panel) = frontend.panel_mut() {
        panel.hide();
    }
    tracing::debug!(target: "typio.panel.host", "panel: hide reason=candidates_empty");
}
