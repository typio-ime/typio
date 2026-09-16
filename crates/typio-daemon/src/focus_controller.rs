//! Desired-vs-actual focus lifecycle controller.
//!
//! There is no stored lifecycle phase. The only persisted things are raw input
//! facts and live resource handles. Every event-loop tick runs one step:
//!
//! ```text
//! facts   = record(inputs)
//! desired = reduce(facts, prev)   pure
//! actual  = observe(resources)    live snapshot
//! effects = diff(desired, actual) pure, minimal, idempotent
//! apply(effects)                  effectful
//! ```
//!
//! This module owns the pure half: facts, desired, actual, effects, `reduce`,
//! and `diff`. The effectful half (observe + apply) reads and mutates the
//! Wayland frontend and lives elsewhere.
//!
//! See `docs/explanation/focus-controller.md` and ADR-0003.

use typio_host_types::InputFacts;

// ── Desired state ────────────────────────────────────────────────────────

/// Whether the session wants a keyboard grab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GrabWant {
    /// No grab wanted.
    #[default]
    None,
    /// Normal deactivate: release keys, reset tracking, but retain the grab
    /// object so the next activation can reuse it (soft pause).
    SoftPause,
    /// Focus is established: grab must exist and be ready for key routing.
    Yes,
}

impl GrabWant {
    /// Pure name helper, for tracing.
    pub fn name(self) -> &'static str {
        match self {
            GrabWant::None => "NONE",
            GrabWant::SoftPause => "SOFT_PAUSE",
            GrabWant::Yes => "YES",
        }
    }
}

/// Desired resource configuration derived from facts.
///
/// `focus_in` / `focus_out` / `reactivate` are edge-triggered: they are true
/// only on the tick when the relevant transition crosses a boundary. This
/// prevents repeated calls while the state is stable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DesiredState {
    pub grab: GrabWant,
    /// YES edge: was not YES before, is YES now. Triggers engine `focus_in`.
    pub focus_in: bool,
    /// YES→non-YES edge: was YES, is not YES now. Triggers engine `focus_out`.
    pub focus_out: bool,
    /// YES→YES with an observed focus boundary: retain the grab and
    /// composition, reset gesture ownership, and re-anchor the panel.
    pub reactivate: bool,
}

// ── Actual state ─────────────────────────────────────────────────────────

/// Unified readiness of the grab + virtual-keyboard-keymap resource. This is
/// a single resource with one state, not a phase plus a separate vk state
/// machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GrabResourceState {
    /// No grab.
    #[default]
    Absent,
    /// Grab exists but the current epoch has not completed the keymap handoff
    /// to the virtual keyboard. Modifier updates may proceed; key presses may
    /// not.
    NeedsKeymap,
    /// Grab exists and the compositor keymap has been forwarded to vk in the
    /// current epoch. Keys may be routed to the engine.
    Ready,
}

impl GrabResourceState {
    /// Pure name helper, for tracing.
    pub fn name(self) -> &'static str {
        match self {
            GrabResourceState::Absent => "ABSENT",
            GrabResourceState::NeedsKeymap => "NEEDS_KEYMAP",
            GrabResourceState::Ready => "READY",
        }
    }
}

/// Read-only snapshot of live resources. Not a second source of truth.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActualState {
    pub connection_alive: bool,
    pub ic_focused: bool,
    pub grab: GrabResourceState,
}

// ── Effects ──────────────────────────────────────────────────────────────

/// Minimal, idempotent effect set produced by [`diff`].
///
/// Applying the same effect set twice is a no-op (or harmless). This is what
/// makes recovery free: suspend, reconnect, and reconcile-repair all funnel
/// into diff → apply rather than bespoke scrub paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectSet {
    pub destroy_grab: bool,
    pub create_grab: bool,
    pub reset_key_routing: bool,
    pub send_focus_in: bool,
    pub send_focus_out: bool,
    pub discard_composition: bool,
    pub clear_preedit: bool,
    pub commit: bool,
    pub reactivate: bool,
}

// ── Pure functions ───────────────────────────────────────────────────────

/// Derive desired state from input facts.
///
/// The final active level determines the resource target. A separately
/// latched boundary distinguishes a field handoff from a text-only update;
/// its lifetime is one observation, independent of protocol `done` batches.
pub fn reduce(facts: &InputFacts, prev: &DesiredState) -> DesiredState {
    let grab = if !facts.connection_alive || facts.suspend_gap_detected || !facts.engine_present {
        GrabWant::None
    } else if facts.im_is_active {
        GrabWant::Yes
    } else if facts.im_focus_changed || prev.grab != GrabWant::None {
        GrabWant::SoftPause
    } else {
        GrabWant::None
    };
    DesiredState {
        grab,
        focus_in: grab == GrabWant::Yes && prev.grab != GrabWant::Yes,
        focus_out: grab != GrabWant::Yes && prev.grab == GrabWant::Yes,
        reactivate: grab == GrabWant::Yes && prev.grab == GrabWant::Yes && facts.im_focus_changed,
    }
}

/// Compute the minimal effect set needed to converge actual onto desired.
///
/// Rules are evaluated independently; multiple can fire on one tick.
pub fn diff(desired: &DesiredState, actual: &ActualState) -> EffectSet {
    let mut e = EffectSet::default();

    // Hard teardown releases the transport and resets router ownership.
    if desired.grab == GrabWant::None && actual.grab != GrabResourceState::Absent {
        e.destroy_grab = true;
        e.reset_key_routing = true;
        e.discard_composition = true;
        e.clear_preedit = true;
        e.commit = true;
    }

    // Creation: we need a grab but it is absent. Covers normal activation,
    // the soft-pause recovery case where the grab was silently dropped while
    // paused, and the "no engine, no grab" degenerate path.
    if (desired.grab == GrabWant::Yes || desired.grab == GrabWant::SoftPause)
        && actual.grab == GrabResourceState::Absent
    {
        e.create_grab = true;
        e.reset_key_routing = true;
    }

    // Focus convergence: edge-triggered focus_in, plus level reconciliation
    // if desired is YES but the input context is not yet focused.
    if desired.focus_in || (desired.grab == GrabWant::Yes && !actual.ic_focused) {
        e.send_focus_in = true;
    }
    // Focus departure: edge-triggered focus_out, plus level reconciliation
    // if desired is not YES but the input context is still marked focused.
    if desired.focus_out || (desired.grab != GrabWant::Yes && actual.ic_focused) {
        e.send_focus_out = true;
        // Leaving an active field abandons any in-flight composition. Discard
        // it engine-side and blank the compositor preedit so a half-typed
        // attempt cannot leak into the next field on its focus_in.
        e.discard_composition = true;
        e.clear_preedit = true;
        e.commit = true;
    }

    // Reactivate: re-anchor the panel to the new caret. The grab and the
    // engine state are preserved.
    if desired.reactivate {
        e.reactivate = true;
    }

    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive() -> InputFacts {
        InputFacts {
            connection_alive: true,
            engine_present: true,
            ..Default::default()
        }
    }

    // ── Reduce: grab want ────────────────────────────────────────────────

    #[test]
    fn reduce_connection_dead_forces_none() {
        let facts = InputFacts {
            connection_alive: false,
            im_focus_changed: true,
            im_is_active: true,
            ..Default::default()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        assert_eq!(reduce(&facts, &prev).grab, GrabWant::None);
    }

    #[test]
    fn reduce_suspend_gap_forces_none() {
        let facts = InputFacts {
            suspend_gap_detected: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        assert_eq!(reduce(&facts, &prev).grab, GrabWant::None);
    }

    #[test]
    fn reduce_deactivate_to_soft_pause() {
        let facts = InputFacts {
            im_focus_changed: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        assert_eq!(reduce(&facts, &prev).grab, GrabWant::SoftPause);
    }

    #[test]
    fn reduce_boundary_ending_inactive_does_not_reactivate() {
        let facts = InputFacts {
            im_focus_changed: true,
            im_is_active: false,
            ..alive()
        };
        let previous = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let desired = reduce(&facts, &previous);
        assert_eq!(desired.grab, GrabWant::SoftPause);
        assert!(desired.focus_out);
        assert!(!desired.reactivate);
    }

    #[test]
    fn reduce_batch_with_activate_and_deactivate_stays_yes() {
        let facts = InputFacts {
            engine_present: true,
            im_focus_changed: true,
            im_is_active: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert_eq!(d.grab, GrabWant::Yes);
        assert!(!d.focus_out);
        assert!(d.reactivate);
    }

    #[test]
    fn reduce_activate_to_yes() {
        let facts = InputFacts {
            im_focus_changed: true,
            im_is_active: true,
            engine_present: true,
            ..alive()
        };
        let prev = DesiredState::default();
        assert_eq!(reduce(&facts, &prev).grab, GrabWant::Yes);
    }

    #[test]
    fn removing_last_engine_releases_an_active_grab() {
        let facts = InputFacts {
            im_is_active: true,
            engine_present: false,
            ..alive()
        };
        let previous = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let desired = reduce(&facts, &previous);
        assert_eq!(desired.grab, GrabWant::None);
        assert!(desired.focus_out);
    }

    #[test]
    fn reduce_activate_without_engine_stays_none() {
        let facts = InputFacts {
            im_focus_changed: true,
            im_is_active: true,
            engine_present: false,
            ..alive()
        };
        let prev = DesiredState::default();
        assert_eq!(reduce(&facts, &prev).grab, GrabWant::None);
    }

    #[test]
    fn reduce_no_event_preserves_prev() {
        let facts = alive();
        let prev = DesiredState {
            grab: GrabWant::SoftPause,
            ..Default::default()
        };
        assert_eq!(reduce(&facts, &prev).grab, GrabWant::SoftPause);
    }

    #[test]
    fn reduce_im_is_active_converges_on_yes_when_engine_present() {
        let facts = InputFacts {
            im_is_active: true,
            engine_present: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::None,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert_eq!(d.grab, GrabWant::Yes);
        assert!(d.focus_in);
    }

    #[test]
    fn reduce_hard_boundary_overrides_deactivate() {
        let facts = InputFacts {
            suspend_gap_detected: true,
            im_focus_changed: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        assert_eq!(reduce(&facts, &prev).grab, GrabWant::None);
    }

    // ── Reduce: focus edge detection ─────────────────────────────────────

    #[test]
    fn reduce_none_to_yes_triggers_focus_in() {
        let facts = InputFacts {
            im_focus_changed: true,
            im_is_active: true,
            engine_present: true,
            ..alive()
        };
        let d = reduce(&facts, &DesiredState::default());
        assert!(d.focus_in);
        assert!(!d.focus_out);
    }

    #[test]
    fn reduce_yes_to_none_triggers_focus_out() {
        let facts = InputFacts {
            im_focus_changed: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert!(!d.focus_in);
        assert!(d.focus_out);
    }

    #[test]
    fn reduce_soft_pause_to_yes_triggers_focus_in() {
        let facts = InputFacts {
            im_focus_changed: true,
            im_is_active: true,
            engine_present: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::SoftPause,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert!(d.focus_in);
        assert!(!d.focus_out);
    }

    #[test]
    fn reduce_stable_yes_no_edge() {
        let facts = InputFacts {
            im_is_active: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert!(!d.focus_in);
        assert!(!d.focus_out);
    }

    #[test]
    fn reduce_reactivate_no_edge() {
        let facts = InputFacts {
            im_focus_changed: true,
            im_is_active: true,
            engine_present: true,
            ..alive()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert!(!d.focus_in);
        assert!(!d.focus_out);
    }

    // ── Diff: grab lifecycle ─────────────────────────────────────────────

    #[test]
    fn diff_none_with_absent_noop() {
        let d = DesiredState::default();
        let a = ActualState::default();
        let e = diff(&d, &a);
        assert!(!e.destroy_grab);
        assert!(!e.create_grab);
    }

    #[test]
    fn diff_none_with_ready_destroys() {
        let d = DesiredState::default();
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(e.destroy_grab);
        assert!(e.discard_composition);
        assert!(e.clear_preedit);
        assert!(e.commit);
    }

    #[test]
    fn diff_yes_with_absent_creates() {
        let d = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let a = ActualState::default();
        let e = diff(&d, &a);
        assert!(e.create_grab);
        assert!(e.reset_key_routing);
    }

    #[test]
    fn diff_soft_pause_with_absent_creates() {
        let d = DesiredState {
            grab: GrabWant::SoftPause,
            ..Default::default()
        };
        let a = ActualState::default();
        let e = diff(&d, &a);
        assert!(e.create_grab);
        assert!(e.reset_key_routing);
    }

    #[test]
    fn diff_soft_pause_with_ready_noop() {
        let d = DesiredState {
            grab: GrabWant::SoftPause,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(!e.destroy_grab);
        assert!(!e.create_grab);
    }

    #[test]
    fn diff_yes_with_needs_keymap_noop() {
        let d = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::NeedsKeymap,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(!e.destroy_grab);
        assert!(!e.create_grab);
    }

    // ── Diff: focus edges ────────────────────────────────────────────────

    #[test]
    fn diff_focus_in_effect() {
        let d = DesiredState {
            grab: GrabWant::Yes,
            focus_in: true,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(e.send_focus_in);
        assert!(!e.send_focus_out);
    }

    #[test]
    fn diff_focus_in_with_retained_grab_does_not_create_grab() {
        let d = DesiredState {
            grab: GrabWant::Yes,
            focus_in: true,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(e.send_focus_in);
        assert!(!e.create_grab);
    }

    #[test]
    fn diff_focus_out_effect() {
        let d = DesiredState {
            grab: GrabWant::SoftPause,
            focus_out: true,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(!e.send_focus_in);
        assert!(e.send_focus_out);
    }

    #[test]
    fn diff_focus_out_discards_composition() {
        let d = DesiredState {
            grab: GrabWant::SoftPause,
            focus_out: true,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(!e.destroy_grab);
        assert!(e.discard_composition);
        assert!(e.clear_preedit);
        assert!(e.commit);
    }

    #[test]
    fn diff_no_focus_change_keeps_composition() {
        let d = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(!e.discard_composition);
        assert!(!e.clear_preedit);
    }

    // ── Diff: idempotency ────────────────────────────────────────────────

    #[test]
    fn diff_idempotent_stable_none() {
        let d = DesiredState::default();
        let a = ActualState::default();
        assert_eq!(diff(&d, &a), diff(&d, &a));
        assert!(!diff(&d, &a).destroy_grab);
        assert!(!diff(&d, &a).create_grab);
    }

    #[test]
    fn diff_idempotent_stable_yes_ready() {
        let d = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ..Default::default()
        };
        assert_eq!(diff(&d, &a), diff(&d, &a));
    }

    // ── Default safety (Rust analog of the C null-pointer tests) ─────────

    #[test]
    fn reduce_default_returns_safe_defaults() {
        // A dead connection (default `connection_alive == false`) forces NONE.
        let d = reduce(&InputFacts::default(), &DesiredState::default());
        assert_eq!(d.grab, GrabWant::None);
        assert!(!d.focus_in);
        assert!(!d.focus_out);
    }

    #[test]
    fn diff_default_returns_empty_effects() {
        let e = diff(&DesiredState::default(), &ActualState::default());
        assert!(!e.destroy_grab);
        assert!(!e.create_grab);
    }

    #[test]
    fn diff_reconciles_unfocused_when_desired_yes() {
        let d = DesiredState {
            grab: GrabWant::Yes,
            focus_in: false,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Ready,
            ic_focused: false,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(e.send_focus_in);
    }

    #[test]
    fn diff_reconciles_focused_when_desired_none() {
        let d = DesiredState {
            grab: GrabWant::None,
            focus_out: false,
            ..Default::default()
        };
        let a = ActualState {
            grab: GrabResourceState::Absent,
            ic_focused: true,
            ..Default::default()
        };
        let e = diff(&d, &a);
        assert!(e.send_focus_out);
        assert!(e.discard_composition);
        assert!(e.clear_preedit);
        assert!(e.commit);
    }

    #[test]
    fn reduce_reactivate_when_boundary_seen_in_stable_yes() {
        let facts = InputFacts {
            connection_alive: true,
            im_focus_changed: true,
            im_is_active: true,
            engine_present: true,
            ..Default::default()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert_eq!(d.grab, GrabWant::Yes);
        assert!(d.reactivate);
        assert!(!d.focus_in);
    }

    #[test]
    fn reduce_text_update_after_consumed_boundary_does_not_reactivate() {
        let facts = InputFacts {
            connection_alive: true,
            im_focus_changed: false,
            im_is_active: true,
            engine_present: true,
            ..Default::default()
        };
        let prev = DesiredState {
            grab: GrabWant::Yes,
            ..Default::default()
        };
        let d = reduce(&facts, &prev);
        assert_eq!(d.grab, GrabWant::Yes);
        assert!(!d.reactivate);
        assert!(!d.focus_in);
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(GrabWant::SoftPause.name(), "SOFT_PAUSE");
        assert_eq!(GrabResourceState::NeedsKeymap.name(), "NEEDS_KEYMAP");
        assert_eq!(GrabResourceState::Ready.name(), "READY");
    }
}
