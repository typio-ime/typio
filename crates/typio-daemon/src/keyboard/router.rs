//! Keyboard event router.
//!
//! Bridges the Wayland input-method keyboard grab to typio-runtime's input
//! context. Decides whether a key is consumed by the engine or forwarded
//! to the focused application via the virtual keyboard.

use std::collections::BTreeSet;
use std::time::Instant;

use typio_runtime::core::engine::{KeyEvent as EngineKeyEvent, KeyState as EngineKeyState};

use super::helpers::{
    commit_candidate_should_fallback, host_selection_plain_key, is_voice_ptt_key,
    page_boundary_selected_index, should_suppress_untracked_modifier_release,
};
use super::output::{PendingComposition, PendingEngineOutput};
use super::preedit_coalescer::{PreeditCoalescer, PreeditUpdate};
use crate::candidate_guard::{
    HostSelectionAction, HostSelectionPageState, classify_host_selection,
};
use crate::input_method::{DecodedKeyEvent, InputMethodState};
use crate::keyboard_policy::{
    KEY_PAGE_DOWN, KEY_PAGE_UP, WL_KEYBOARD_KEY_STATE_PRESSED, WL_KEYBOARD_KEY_STATE_RELEASED,
    effective_modifiers, modifier_bit_for_keysym, repeat_should_cancel_on_modifier_transition,
};
use crate::text_ui_state::{PreeditTracking, TextUiPlan, text_ui_plan_update};
use typio_host_types::{Modifiers, should_repeat_for_modifiers};

/// Result of [`KeyboardRouter::dispatch_repeat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatOutcome {
    /// The engine consumed the repeat event. The caller should drain
    /// any resulting commit/composition output.
    Consumed,
    /// The repeat chain has ended — either no key is pending or the
    /// engine declined the repeat. The caller should stop the timer.
    Stopped,
}

/// A keyboard router that owns one typio-runtime input context.
pub struct KeyboardRouter {
    ctx: Option<Box<typio_runtime::TypioInputContext>>,
    /// Output drained from the owned context and staged until the host flushes
    /// it; no global lock or cross-context state.
    pending_output: PendingEngineOutput,
    /// Key currently held down and subject to auto-repeat, if any.
    /// Set on the initial press (whether the key was consumed by the
    /// engine or forwarded to the application) and cleared on release.
    repeat_key: Option<DecodedKeyEvent>,
    /// Modifier mask sampled when the repeat chain was armed. A change in
    /// the blocking modifiers (Ctrl/Alt/Super) after arming ends the chain
    /// — the held key has become a chord, or a coalesced `Modifiers` event
    /// corrected a stale arm-time sample (the "wwwwww" regression).
    repeat_arm_mods: Modifiers,
    /// Physical modifier state tracked from key events.
    pub(crate) physical_modifiers: Modifiers,
    /// Whether `physical_modifiers` has been seeded from the first xkb
    /// modifier sample of this grab generation. The compositor does not
    /// re-report modifiers that were already held when the grab started,
    /// so without a one-time baseline a Shift or Super held across the
    /// grab boundary would be invisible to the engine. See
    /// [`Self::acquire_modifiers_if_needed`].
    modifiers_acquired: bool,
    /// Presses routed in this focus epoch. Releases without ownership never
    /// enter an engine or complete a shortcut in a different field.
    pressed_keys: BTreeSet<u32>,
    /// Gesture latch: set when any non-modifier key is pressed during the
    /// current switch gesture, cleared only at gesture boundaries (the
    /// first chord modifier going down from a clean state, and the last
    /// one coming up). Unlike a "currently held" flag this stays latched
    /// after the key lifts — so Ctrl+Shift+V suppresses the switch even
    /// when V is released before the modifiers. Drives the release-time
    /// chord decision in [`Self::track_switch_modifier`].
    shortcut_saw_non_modifier: bool,
    /// True once the full required modifier set (Ctrl+Shift) was held
    /// simultaneously during the current gesture. The switch fires on the
    /// *release* that breaks this set, not on the press that completes it,
    /// so a chord that turns out to be the prefix of an app shortcut
    /// (Ctrl+Shift+V) never fires prematurely.
    chord_full_set_held: bool,
    /// True if the switch chord already fired in this gesture; prevents
    /// repeat triggers while the modifiers stay held.
    shortcut_already_triggered: bool,
    /// Set when a chord completes during `dispatch_key`. The main loop
    /// drains this and cycles the active keyboard engine.
    shortcut_fired: bool,
    /// Modifiers whose most recent press was forwarded to the engine
    /// rather than swallowed by chord suppression. Used to forward
    /// releases symmetrically so the engine never observes an unpaired
    /// modifier release (which could spuriously toggle state, e.g. a
    /// Rime schema switch on a Shift release whose press was consumed
    /// by the Ctrl+Shift switch chord).
    engine_tracked_mods: Modifiers,
    /// Last preedit the host actually sent to the compositor. Used by
    /// [`Self::drain_composition`] to suppress redundant
    /// `set_preedit_string` + `commit` Wayland round-trips when the
    /// engine reports the same preedit text and cursor as the previous
    /// composition (the canonical case: Up/Down arrow navigation moves
    /// only the candidate highlight, leaving the inline preedit text
    /// untouched). See [`crate::text_ui_state::text_ui_plan_update`].
    preedit_tracking: PreeditTracking,
    /// Commit text waiting to be sent in the next text-input transaction.
    pending_commit_flush: Option<String>,
    /// Latest preedit update waiting to be committed to Wayland. The bounded
    /// coalescer spans adjacent reactor steps because physically adjacent keys
    /// are not guaranteed to share one pending-key drain. Candidate state is
    /// still updated immediately in host memory; only the compositor-facing
    /// text transaction waits for the quiet/hard deadline.
    preedit_coalescer: PreeditCoalescer,
    /// Keycode currently held as the voice push-to-talk trigger, if any.
    voice_ptt_keycode: Option<u32>,
    /// Latched when `Super+V` starts voice push-to-talk.
    voice_ptt_pressed: bool,
    /// Latched when the matching `V` release ends voice push-to-talk.
    voice_ptt_released: bool,
}

/// Default engine-switch chord: Ctrl+Shift, the standard Linux IME
/// switch. Both modifiers must be held simultaneously (any side, any
/// order); the chord fires when they are *released* without any other
/// key having been pressed — so Ctrl+Shift+V and similar app shortcuts
/// pass through untouched.
pub fn default_switch_binding() -> crate::keyboard_policy::ShortcutBinding {
    use crate::keyboard_policy::ShortcutBinding;
    ShortcutBinding {
        modifiers: Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0),
        keysym: 0, // unused — chord_is_switch_modifier covers both sides
    }
}
impl KeyboardRouter {
    /// Create a new router for the given TypioInstance.
    ///
    pub fn new(instance: &mut typio_runtime::TypioInstance) -> Self {
        let ctx = typio_runtime::TypioInputContext::new_rust(instance);
        Self {
            ctx: Some(ctx),
            pending_output: PendingEngineOutput::default(),
            repeat_key: None,
            repeat_arm_mods: Modifiers::NONE,
            physical_modifiers: Modifiers::NONE,
            modifiers_acquired: false,
            pressed_keys: BTreeSet::new(),
            shortcut_saw_non_modifier: false,
            chord_full_set_held: false,
            shortcut_already_triggered: false,
            shortcut_fired: false,
            engine_tracked_mods: Modifiers::NONE,
            preedit_tracking: PreeditTracking::new(),
            pending_commit_flush: None,
            preedit_coalescer: PreeditCoalescer::default(),
            voice_ptt_keycode: None,
            voice_ptt_pressed: false,
            voice_ptt_released: false,
        }
    }

    /// Notify the engine that the input context has gained focus.
    pub fn focus_in(&mut self) {
        if let Some(ctx) = self.ctx.as_mut() {
            ctx.focus_in();
        }
    }

    /// Notify the engine that the input context has lost focus.
    pub fn focus_out(&mut self) {
        if let Some(ctx) = self.ctx.as_mut() {
            ctx.focus_out();
        }
    }

    /// Forget the last preedit we claimed to have sent to the compositor.
    /// Called by the focus controller whenever it clears the visible
    /// preedit through a path other than `drain_composition`, so the
    /// next composition is not suppressed against stale tracking.
    pub fn preedit_tracking_reset(&mut self) {
        self.preedit_tracking.reset();
        self.pending_commit_flush = None;
        self.preedit_coalescer.clear();
    }

    /// True iff the runtime input context currently reports itself focused.
    pub fn is_focused(&self) -> bool {
        self.ctx.as_deref().is_some_and(|ctx| ctx.is_focused())
    }

    pub fn has_context(&self) -> bool {
        self.ctx.is_some()
    }

    /// Reset the engine's in-flight composition and candidate state.
    pub fn reset(&mut self) {
        if let Some(ctx) = self.ctx.as_mut() {
            ctx.reset();
        }
        self.capture_engine_output();
        self.pending_output.commit = None;
        self.pending_output.composition = None;
        self.pending_commit_flush = None;
        self.preedit_coalescer.clear();
        // Forget any preedit we claimed to have sent — the engine reset
        // may be followed by the compositor clearing the field on its
        // own, and the next composition must not be suppressed as a
        // "no-op" against stale tracking.
        self.preedit_tracking.reset();
    }

    /// Drain any pending composition update and update preedit/candidates.
    pub fn drain_composition(&mut self, frontend: &mut InputMethodState, now: Instant) {
        self.capture_engine_output();
        if let Some(pending) = self.pending_output.composition.take() {
            let PendingComposition {
                preedit_text: preedit,
                cursor_pos,
                candidates,
                selected,
                has_prev,
                has_next,
                host_managed_selection,
            } = pending;
            let preedit_len = preedit.len();
            let candidate_count = candidates.len();
            // Mirror the engine's declared selection-intercept flags
            // into the state so the next `dispatch_key` can apply
            // candidate_guard without consulting the engine again.
            frontend.composition.host_managed_selection = host_managed_selection;
            // Preedit is the source of truth for what shows inline in
            // the focused text field. Candidates drive the popup. Either
            // can change independently of the other: an empty preedit
            // with non-empty candidates means the engine is offering
            // completions; a non-empty preedit with no candidates means
            // the engine is mid-composition (e.g. pinyin after one
            // keystroke) and will show candidates later.
            if preedit.is_empty() && candidates.is_empty() {
                // Both cleared. Only re-clear if we actually had a
                // preedit outstanding; otherwise this would emit a
                // `set_preedit_string("") + commit` Wayland round-trip
                // on every composition update where the engine reports
                // "nothing to show" (e.g. after every commit).
                if self.preedit_tracking.last_text.is_some()
                    || self.preedit_tracking.last_cursor != -1
                    || self.preedit_coalescer.pending().is_some()
                {
                    self.preedit_coalescer.stage(
                        PreeditUpdate {
                            text: String::new(),
                            cursor: 0,
                            engine_cursor_pos: -1,
                        },
                        now,
                    );
                }
            } else {
                // Resolve the engine's cursor_pos (non-negative wins,
                // negative falls back to end) so left/right navigation
                // inside the preedit actually moves the visible caret
                // instead of always parking at the right edge.
                let cursor = crate::preedit::resolve_cursor(cursor_pos, &preedit) as u32;
                // Compare against what we last actually sent to the
                // compositor. Up/Down candidate navigation is the
                // canonical case where the engine emits a composition
                // with identical preedit text + cursor and a different
                // `selected` — re-sending the preedit there is pure
                // waste (a `set_preedit_string` + `commit` Wayland
                // round-trip per arrow press).
                let effective_last_text = self
                    .preedit_coalescer
                    .pending()
                    .map(|p| p.text.as_str())
                    .or(self.preedit_tracking.last_text.as_deref());
                let effective_last_cursor = self
                    .preedit_coalescer
                    .pending()
                    .map(|p| p.engine_cursor_pos)
                    .unwrap_or(self.preedit_tracking.last_cursor);
                let plan = text_ui_plan_update(
                    effective_last_text,
                    effective_last_cursor,
                    Some(preedit.as_str()),
                    cursor_pos,
                );
                if plan == TextUiPlan::SyncPreeditAndPanel {
                    self.preedit_coalescer.stage(
                        PreeditUpdate {
                            text: preedit.clone(),
                            cursor,
                            engine_cursor_pos: cursor_pos,
                        },
                        now,
                    );
                }
                // SyncPanelOnly: skip the Wayland round-trip; the
                // candidate-panel repaint below is driven independently
                // by `mark_panel_dirty`.
            }
            let composition_seq = frontend.set_candidates(candidates, selected);
            frontend
                .composition
                .set_candidate_page_state(has_prev, has_next);
            frontend.mark_panel_dirty();
            tracing::debug!(
                target: "typio.engine.composition",
                composition_seq,
                preedit_len,
                cursor_pos,
                candidate_count,
                selected,
                "composition update"
            );
        }
    }

    /// Abandon routing and staged text when the input context leaves focus.
    pub fn soft_pause(&mut self) {
        self.fence_key_routing();
        self.pending_commit_flush = None;
        self.preedit_coalescer.clear();
    }

    /// Reset gesture ownership. Virtual-keyboard releases belong to the
    /// platform's ordered boundary event, not a second router ledger.
    pub fn fence_key_routing(&mut self) {
        self.pressed_keys.clear();
        self.cancel_repeat();
        self.physical_modifiers = Modifiers::NONE;
        self.modifiers_acquired = false;
        self.engine_tracked_mods = Modifiers::NONE;
        self.shortcut_saw_non_modifier = false;
        self.chord_full_set_held = false;
        self.shortcut_already_triggered = false;
        self.shortcut_fired = false;
        self.voice_ptt_keycode = None;
        self.voice_ptt_pressed = false;
        self.voice_ptt_released = false;
    }

    pub fn reset_key_routing(&mut self) {
        self.soft_pause();
    }

    /// True iff the configured engine-switch chord (Ctrl+Shift by
    /// default) completed during the most recent `dispatch_key`. The
    /// main loop drains this once per reactor step and cycles the active
    /// keyboard. Reading clears the flag.
    pub fn take_switch_chord_fired(&mut self) -> bool {
        std::mem::take(&mut self.shortcut_fired)
    }

    /// True iff the most recent dispatch started voice push-to-talk.
    pub fn take_voice_ptt_pressed(&mut self) -> bool {
        std::mem::take(&mut self.voice_ptt_pressed)
    }

    /// True iff the most recent dispatch ended voice push-to-talk.
    pub fn take_voice_ptt_released(&mut self) -> bool {
        std::mem::take(&mut self.voice_ptt_released)
    }

    /// Update the surrounding text snapshot received from Wayland.
    pub fn set_surrounding(&mut self, text: &str, cursor: i32, anchor: i32) {
        if let Some(ctx) = self.ctx.as_mut() {
            ctx.set_surrounding(text, cursor, anchor);
        }
    }

    /// Try to handle `key` through host-managed candidate selection
    /// (ADR-0012), bypassing the engine's `process_key`.
    ///
    /// Returns `Some(handled)` when the host took the key (either by
    /// moving the highlight locally — navigation — or by dispatching
    /// `typio_input_context_commit_candidate` — commit). Returns
    /// `None` when the host did not take the key and the caller should
    /// fall back to the engine via [`Self::dispatch_key`].
    ///
    /// **Opt-in.** The engine must have declared a non-empty
    /// `host_managed_selection` flag set in its last composition; see
    /// [`crate::candidate_guard::should_consume_key`]. Engines that
    /// have not opted in are not affected.
    ///
    /// For **navigation** keys (Up/Down/Left/Right) the highlight is
    /// moved locally and the panel is marked dirty. The engine never
    /// sees the key — this is the canonical perf win of host-managed
    /// selection: no synchronous FFI round-trip per arrow press.
    ///
    /// For **commit** keys (Space / digits / Enter-raw) the host calls
    /// `typio_input_context_commit_candidate` so the engine can
    /// dispatch its own `commit_candidate` vtable entry. If the engine
    /// declines (returns `TypioErrorNotFound` — typically because it
    /// doesn't implement the vtable entry), the host returns `None` so
    /// the caller can fall back to `process_key`, preserving the
    /// user's intent instead of dropping the key.
    ///
    /// Release events under host-managed selection are swallowed
    /// (`Some(true)`) so the engine never observes an unpaired release
    /// for a press it didn't see.
    pub fn try_host_selection(
        &mut self,
        key: &DecodedKeyEvent,
        frontend: &mut InputMethodState,
        xkb_mods_depressed: u32,
        now: Instant,
    ) -> Option<bool> {
        if !host_selection_plain_key(self.shortcut_modifiers(xkb_mods_depressed)) {
            return None;
        }
        let action = classify_host_selection(
            key.state == WL_KEYBOARD_KEY_STATE_PRESSED,
            key.keysym,
            frontend.composition.candidates.len(),
            frontend.composition.selected_candidate,
            frontend.composition.host_managed_selection,
            HostSelectionPageState {
                has_prev: frontend.composition.has_prev_candidates,
                has_next: frontend.composition.has_next_candidates,
            },
        )?;
        match action {
            HostSelectionAction::Swallow => Some(true),
            HostSelectionAction::Navigate(new_idx) => {
                if new_idx != frontend.composition.selected_candidate
                    && !self.sync_host_candidate_selection(
                        frontend,
                        new_idx,
                        "host-managed navigation",
                    )
                {
                    return None;
                }
                Some(true)
            }
            HostSelectionAction::Commit(idx) => {
                let had_pending_output = self.pending_output.commit.is_some()
                    || self.pending_output.composition.is_some();
                let r = self
                    .ctx
                    .as_mut()
                    .ok_or(typio_runtime::core::engine::EngineError::NotFound)
                    .and_then(|ctx| ctx.commit_candidate(idx as i32));
                self.capture_engine_output();
                let produced_engine_output = !had_pending_output
                    && (self.pending_output.commit.is_some()
                        || self.pending_output.composition.is_some());
                if r.is_ok() || !commit_candidate_should_fallback(&r, produced_engine_output) {
                    return Some(true);
                }
                tracing::debug!(
                    target: "typio.engine.host_sel",
                    idx,
                    result = ?r,
                    "commit_candidate failed without output; falling back to process_key"
                );
                // Engine failed without producing any observable output
                // (typical: no vtable entry). Fall back to process_key so
                // the user's intent isn't lost. If the engine already emitted
                // commit/composition output before returning an error, the
                // selection was consumed (Rime partial selection can do this)
                // and replaying the same Space/digit would immediately select
                // the first candidate of the remaining segment.
                None
            }
            HostSelectionAction::PageUp => Some(self.dispatch_synthetic_page_key(
                key,
                KEY_PAGE_UP,
                frontend,
                "host-managed page up",
                now,
            )),
            HostSelectionAction::PageDown => Some(self.dispatch_synthetic_page_key(
                key,
                KEY_PAGE_DOWN,
                frontend,
                "host-managed page down",
                now,
            )),
        }
    }

    fn sync_host_candidate_selection(
        &mut self,
        frontend: &mut InputMethodState,
        selected: usize,
        label: &'static str,
    ) -> bool {
        let r = self
            .ctx
            .as_mut()
            .ok_or(typio_runtime::core::engine::EngineError::NotFound)
            .and_then(|ctx| ctx.set_candidate_selection(selected));
        if r.is_err() {
            tracing::debug!(
                target: "typio.engine.host_sel",
                selected,
                result = ?r,
                "failed to sync host-managed candidate selection"
            );
            return false;
        }
        frontend.composition.selected_candidate = selected;
        frontend.composition.composition_seq = frontend.composition.composition_seq.wrapping_add(1);
        frontend.mark_panel_dirty();
        tracing::trace!(
            target: "typio.engine.host_sel",
            selected,
            label
        );
        true
    }

    fn dispatch_synthetic_page_key(
        &mut self,
        source: &DecodedKeyEvent,
        keysym: u32,
        frontend: &mut InputMethodState,
        label: &'static str,
        now: Instant,
    ) -> bool {
        let page_key = DecodedKeyEvent {
            keycode: source.keycode,
            xkb_keycode: source.xkb_keycode,
            keysym,
            unicode: String::new(),
            state: WL_KEYBOARD_KEY_STATE_PRESSED,
            time: source.time,
        };
        let consumed = self.process_key_engine(&page_key, 0, false);
        self.drain_composition(frontend, now);
        if consumed {
            if let Some(selected) =
                page_boundary_selected_index(keysym, frontend.composition.candidates.len())
            {
                if selected != frontend.composition.selected_candidate {
                    let _ = self.sync_host_candidate_selection(
                        frontend,
                        selected,
                        "host-managed page boundary selection",
                    );
                }
            }
        }
        tracing::trace!(
            target: "typio.engine.host_sel",
            keysym,
            consumed,
            label
        );
        consumed
    }

    /// Dispatch one decoded key event to the engine.
    ///
    /// Returns `true` if the engine consumed the key. The event reaches
    /// the engine with `is_repeat: false`; repeats are driven by the
    /// main loop's repeat timer via [`Self::dispatch_repeat`].
    pub fn dispatch_key(&mut self, key: &DecodedKeyEvent, xkb_mods_depressed: u32) -> bool {
        let state = if key.state == 1 {
            WL_KEYBOARD_KEY_STATE_PRESSED
        } else {
            WL_KEYBOARD_KEY_STATE_RELEASED
        };

        // Seed physical modifier state from the first xkb sample of this
        // generation. Done before the per-key tracking below so that any
        // modifier already held when the grab started (or whose Modifiers
        // event raced ahead of its Key event) is visible to both the chord
        // state machine and the engine. Idempotent within a generation.
        self.acquire_modifiers_if_needed(xkb_mods_depressed);

        // Update physical modifier tracking.
        let bit = modifier_bit_for_keysym(key.keysym);
        let is_modifier_key = bit != Modifiers::NONE;
        // Whether to suppress this modifier's release from the engine.
        // A release is only forwarded when the matching press was also
        // forwarded; chord-suppressed presses get chord-suppressed
        // releases so the engine never sees an unpaired event that
        // could spuriously toggle state (e.g. Rime schema switch).
        let mut suppress_engine_release = false;
        if is_modifier_key {
            // Drive the release-triggered switch-chord state machine. It
            // updates `physical_modifiers` and reports whether *this*
            // release completes the Ctrl+Shift switch.
            let chord_fired =
                self.track_switch_modifier(bit, state == WL_KEYBOARD_KEY_STATE_PRESSED);
            if chord_fired {
                self.shortcut_fired = true;
            }
            if state == WL_KEYBOARD_KEY_STATE_RELEASED {
                if (self.engine_tracked_mods.0 & bit.0) == 0
                    && should_suppress_untracked_modifier_release(
                        bit,
                        self.shortcut_modifiers(xkb_mods_depressed),
                    )
                {
                    suppress_engine_release = true;
                }
                self.engine_tracked_mods = Modifiers(self.engine_tracked_mods.0 & !bit.0);
            }
        } else {
            // Any non-modifier key pressed during a gesture taints it, so
            // the trailing Ctrl+Shift release won't switch (Ctrl+Shift+V
            // pastes). The modifier key still forwards to the engine /
            // app below.
            if state == WL_KEYBOARD_KEY_STATE_PRESSED {
                self.shortcut_saw_non_modifier = true;
            }
        }

        if state == WL_KEYBOARD_KEY_STATE_RELEASED && self.voice_ptt_keycode == Some(key.keycode) {
            self.voice_ptt_keycode = None;
            self.voice_ptt_released = true;
            return true;
        }

        let mods = self.shortcut_modifiers(xkb_mods_depressed);
        if state == WL_KEYBOARD_KEY_STATE_PRESSED
            && !is_modifier_key
            && is_voice_ptt_key(key.keysym)
            && mods.intersects(Modifiers::SUPER)
            && !mods.intersects(Modifiers(Modifiers::CTRL.0 | Modifiers::ALT.0))
        {
            self.voice_ptt_keycode = Some(key.keycode);
            self.voice_ptt_pressed = true;
            return true;
        }

        if key.state != 1 {
            // Release: forward to the engine so engines that need
            // release events (e.g. Rime schema switching on a lone
            // Shift release) can complete gesture detection — unless
            // the matching press was chord-suppressed.
            if suppress_engine_release {
                return false;
            }
            let consumed = self.process_key_engine(key, xkb_mods_depressed, false);
            return consumed;
        }

        // Press: record the modifier as engine-tracked before
        // forwarding so its later release can be paired.
        if is_modifier_key {
            self.engine_tracked_mods = Modifiers(self.engine_tracked_mods.0 | bit.0);
        }
        self.process_key_engine(key, xkb_mods_depressed, false)
    }

    fn shortcut_modifiers(&self, xkb_mods_depressed: u32) -> Modifiers {
        Modifiers(self.physical_modifiers.0 | xkb_mods_depressed)
    }

    /// Advance the Ctrl+Shift engine-switch chord state machine for one
    /// modifier-key transition, updating [`Self::physical_modifiers`].
    /// Returns `true` iff this is the *release* that completes the switch.
    ///
    /// The switch is release-triggered, not press-triggered: it fires when
    /// a chord modifier comes up after the full set was held, provided no
    /// non-modifier key joined the gesture. Firing on the completing press
    /// instead would trip on the prefix of app shortcuts — pressing Shift
    /// while Ctrl is down would switch engines before the user ever pressed
    /// `V` in Ctrl+Shift+V.
    ///
    /// A gesture spans from the first required modifier going down (from a
    /// state with none held) to the last one coming up; the latch and
    /// armed flags reset at both boundaries.
    fn track_switch_modifier(&mut self, bit: Modifiers, pressed: bool) -> bool {
        let chord_mods = default_switch_binding().modifiers;
        let is_chord_mod = (chord_mods.0 & bit.0) != 0;

        if pressed {
            // A chord modifier going down while none of the required
            // modifiers were held starts a fresh gesture.
            if is_chord_mod && (self.physical_modifiers.0 & chord_mods.0) == 0 {
                self.shortcut_saw_non_modifier = false;
                self.chord_full_set_held = false;
                self.shortcut_already_triggered = false;
            }
            self.physical_modifiers = Modifiers(self.physical_modifiers.0 | bit.0);
            if (self.physical_modifiers.0 & chord_mods.0) == chord_mods.0 {
                self.chord_full_set_held = true;
            }
            false
        } else {
            // Fire on the first chord-modifier release that breaks a fully
            // held set, when the gesture is untainted and hasn't fired yet.
            let fired = is_chord_mod
                && self.chord_full_set_held
                && !self.shortcut_saw_non_modifier
                && !self.shortcut_already_triggered;
            if fired {
                self.shortcut_already_triggered = true;
            }
            self.physical_modifiers = Modifiers(self.physical_modifiers.0 & !bit.0);
            // Gesture ends once no required modifier remains held.
            if (self.physical_modifiers.0 & chord_mods.0) == 0 {
                self.shortcut_saw_non_modifier = false;
                self.chord_full_set_held = false;
                self.shortcut_already_triggered = false;
            }
            fired
        }
    }

    /// Dispatch a key-repeat event to the engine with `is_repeat: true`.
    ///
    /// This skips the modifier tracking, chord detection, and release
    /// early-return that [`Self::dispatch_key`] performs — none of those
    /// apply to a synthetic repeat (the physical state hasn't changed
    /// since the initial press). Returns `true` if the engine consumed
    /// the repeat.
    fn dispatch_key_repeat(&mut self, key: &DecodedKeyEvent, xkb_mods_depressed: u32) -> bool {
        self.process_key_engine(key, xkb_mods_depressed, true)
    }

    /// Seed [`Self::physical_modifiers`] from the first xkb-derived
    /// modifier sample seen in the current grab generation.
    ///
    /// The Wayland grab does not replay the modifier state that was
    /// already held when the grab began, nor does it guarantee that a
    /// `Modifiers` event arrives before the `Key` event it pertains to.
    /// Without a baseline, a modifier held across the boundary — or one
    /// whose `Modifiers` event raced ahead of its `Key` event — would be
    /// invisible to the engine, causing it to misclassify chords. The
    /// canonical failure is a lone-Shift mode toggle firing on
    /// Super+Shift: the Shift release reaches the engine with an empty
    /// mask because Super was never seeded.
    ///
    /// After this one-time seeding, [`Self::track_switch_modifier`]
    /// keeps `physical_modifiers` correct on its own, transition by
    /// transition. The `active_generation_owned_keys` argument passed to
    /// [`effective_modifiers`] is therefore `true`: once seeded we own
    /// the per-key modifier tracking and should not fold the (now
    /// possibly-stale) xkb blocking bits back in.
    fn acquire_modifiers_if_needed(&mut self, xkb_mods_depressed: u32) {
        if self.modifiers_acquired {
            return;
        }
        self.modifiers_acquired = true;
        // OR the blocking bits into whatever physical tracking already
        // recorded (in case an earlier modifier key press in this same
        // dispatch ran first). Non-blocking bits (locks) stay owned by
        // xkb and are applied per-event in effective_modifiers.
        let blocking = Modifiers(
            Modifiers::SHIFT.0 | Modifiers::CTRL.0 | Modifiers::ALT.0 | Modifiers::SUPER.0,
        );
        self.physical_modifiers =
            Modifiers(self.physical_modifiers.0 | (xkb_mods_depressed & blocking.0));
    }

    /// Compute the modifier mask the engine should see for `key`.
    ///
    /// This is the pure, FFI-free heart of [`Self::process_key_engine`],
    /// factored out so it can be unit-tested without a live runtime
    /// context.
    ///
    /// We pass `active_generation_owned_keys = true`: once the first
    /// event of a generation has seeded physical state (via
    /// [`Self::acquire_modifiers_if_needed`]) and
    /// [`Self::track_switch_modifier`] maintains it transition by
    /// transition, the host's per-key tracking is authoritative for the
    /// blocking modifiers (Shift/Ctrl/Alt/Super). Folding the xkb-derived
    /// blocking bits back in here — as the former
    /// `sync_physical_modifiers` did — would let a stale per-step xkb
    /// snapshot erase a still-held sibling modifier. That is the root
    /// cause of Super+Shift being misread as a lone Shift: the Shift
    /// release reached the engine carrying no Super bit, so the engine
    /// toggled mode. `effective_modifiers` with owned=true keeps the
    /// sibling's bit, so the engine sees the full chord and declines to
    /// toggle. Locks (Caps/Num) are still taken from xkb.
    fn engine_modifier_mask(&self, key: &DecodedKeyEvent, xkb_mods_depressed: u32) -> Modifiers {
        effective_modifiers(
            self.physical_modifiers,
            Modifiers(xkb_mods_depressed),
            true,
            key.keysym,
            if key.state == 1 {
                WL_KEYBOARD_KEY_STATE_PRESSED
            } else {
                WL_KEYBOARD_KEY_STATE_RELEASED
            },
        )
    }

    /// Shared engine-dispatch core used by both the initial-press and
    /// repeat paths. Builds the `TypioKeyEvent` with the supplied
    /// `is_repeat` flag and forwards it to the active engine backend.
    fn process_key_engine(
        &mut self,
        key: &DecodedKeyEvent,
        xkb_mods_depressed: u32,
        is_repeat: bool,
    ) -> bool {
        let effective = self.engine_modifier_mask(key, xkb_mods_depressed);

        let event = EngineKeyEvent {
            sym: typio_runtime::core::engine::KeySym::Raw(key.keysym),
            state: if key.state == 1 {
                EngineKeyState::Press
            } else {
                EngineKeyState::Release
            },
            code: key.keycode,
            modifiers: effective.0,
            unicode: key.unicode.chars().next().unwrap_or('\0') as u32,
            time: key.time as u64,
            is_repeat,
            base_keysym: key.keysym,
        };

        let timing_enabled = tracing::enabled!(target: "typio.engine.key", tracing::Level::TRACE)
            || tracing::enabled!(target: "typio.engine.key", tracing::Level::INFO);
        let started = timing_enabled.then(std::time::Instant::now);
        let consumed = self.ctx.as_mut().is_some_and(|ctx| ctx.process_key(&event));
        if let Some(started) = started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms > 5.0 {
                tracing::info!(
                    target: "typio.engine.key",
                    elapsed_ms,
                    keysym = key.keysym,
                    keycode = key.keycode,
                    pressed = key.state == 1,
                    is_repeat,
                    consumed,
                    "slow process_key"
                );
            } else {
                tracing::trace!(
                    target: "typio.engine.key",
                    elapsed_ms,
                    keysym = key.keysym,
                    keycode = key.keycode,
                    pressed = key.state == 1,
                    is_repeat,
                    consumed,
                    "process_key"
                );
            }
        }
        consumed
    }

    /// Drain any pending commit text from the engine callback into the next
    /// text-input transaction.  The transaction is flushed by
    /// [`Self::flush_pending_text`] after the matching composition has also had
    /// a chance to contribute a replacement preedit.
    pub fn drain_commit(&mut self) {
        self.capture_engine_output();
        if let Some(text) = self.pending_output.commit.take() {
            // A commit replaces the existing preedit with the cursor before
            // inserting text.  Any deferred composition-only preedit from an
            // earlier key is now obsolete; a same-response remaining preedit
            // will be staged by drain_composition immediately after this.
            self.preedit_coalescer.clear();
            self.pending_commit_flush = Some(text);
        }
    }

    fn capture_engine_output(&mut self) {
        let Some(ctx) = self.ctx.as_mut() else {
            return;
        };
        let events = ctx.drain_events().collect::<Vec<_>>();
        for event in events {
            self.pending_output.absorb(event);
        }
    }

    /// Flush the staged text-input transaction, if any.
    fn flush_pending_text(&mut self, frontend: &mut InputMethodState) {
        if self.pending_commit_flush.is_none() && self.preedit_coalescer.pending().is_none() {
            return;
        }

        let commit = self.pending_commit_flush.take();
        let preedit = self.preedit_coalescer.take();
        self.send_text_transaction(frontend, commit, preedit);
    }

    /// Commit immediately when the engine produced real commit text; this
    /// preserves ordering before the next key is routed. Pure preedit updates
    /// remain staged so adjacent reactor steps can coalesce to one Wayland
    /// transaction.
    pub fn flush_pending_text_if_commit(&mut self, frontend: &mut InputMethodState) {
        if self.pending_commit_flush.is_some() {
            self.flush_pending_text(frontend);
        }
    }

    /// Flush pure preedit only after its bounded coalescing deadline. Commit
    /// text continues to bypass the deadline through
    /// [`Self::flush_pending_text_if_commit`].
    pub fn flush_pending_text_if_due(&mut self, frontend: &mut InputMethodState, now: Instant) {
        if self.pending_commit_flush.is_some() {
            self.flush_pending_text(frontend);
            return;
        }
        let Some(preedit) = self.preedit_coalescer.take_if_due(now) else {
            return;
        };
        self.send_text_transaction(frontend, None, Some(preedit));
    }

    /// Remaining bounded-preedit delay for integration into the main poll
    /// timeout. `None` means no pure preedit is staged.
    pub fn preedit_deadline_remaining_ms(&self, now: Instant) -> Option<i32> {
        self.preedit_coalescer.deadline_remaining_ms(now)
    }

    fn send_text_transaction(
        &mut self,
        frontend: &mut InputMethodState,
        commit: Option<String>,
        preedit: Option<PreeditUpdate>,
    ) {
        frontend.text_transaction_and_flush(
            commit.as_deref(),
            preedit.as_ref().map(|p| (p.text.as_str(), p.cursor)),
        );

        match preedit {
            Some(p) if p.text.is_empty() => self.preedit_tracking.reset(),
            Some(p) => {
                self.preedit_tracking.last_text = Some(p.text);
                self.preedit_tracking.last_cursor = p.engine_cursor_pos;
            }
            None if commit.is_some() => self.preedit_tracking.reset(),
            None => {}
        }
    }

    /// Claim a physical press before host shortcuts or engine routing.
    pub fn on_press(&mut self, keycode: u32) {
        self.pressed_keys.insert(keycode);
    }

    pub fn owns_press(&self, keycode: u32) -> bool {
        self.pressed_keys.contains(&keycode)
    }

    /// Only consumed presses use the host timer. Forwarded presses use the
    /// application's native repeat, paired by the virtual-keyboard ledger.
    pub fn on_consumed(&mut self, key: DecodedKeyEvent, arm_mods: Modifiers) {
        self.repeat_key = Some(key);
        self.repeat_arm_mods = arm_mods;
    }

    pub fn cancel_repeat(&mut self) {
        self.repeat_key = None;
        self.repeat_arm_mods = Modifiers::NONE;
    }

    /// Returns whether this release stopped the currently repeating key.
    pub fn on_release(&mut self, key: &DecodedKeyEvent) -> bool {
        self.pressed_keys.remove(&key.keycode);
        let stopped = self
            .repeat_key
            .as_ref()
            .is_some_and(|held| held.keycode == key.keycode);
        if stopped {
            self.cancel_repeat();
        }
        stopped
    }

    /// A modifier held before this epoch can lift without an owned press.
    /// Update its physical baseline without completing an old shortcut.
    pub fn on_unowned_release(&mut self, key: &DecodedKeyEvent) {
        let bit = modifier_bit_for_keysym(key.keysym);
        self.physical_modifiers = Modifiers(self.physical_modifiers.0 & !bit.0);
        self.engine_tracked_mods = Modifiers(self.engine_tracked_mods.0 & !bit.0);
    }

    pub fn observe_modifiers(&mut self, mods: Modifiers) -> bool {
        let cancel = self.repeat_key.is_some()
            && repeat_should_cancel_on_modifier_transition(self.repeat_arm_mods, mods);
        if cancel {
            self.cancel_repeat();
        }
        cancel
    }

    fn repeat_guard_allows(&self, keycode: u32, current_mods: Modifiers) -> bool {
        self.owns_press(keycode)
            && should_repeat_for_modifiers(current_mods)
            && !repeat_should_cancel_on_modifier_transition(self.repeat_arm_mods, current_mods)
    }

    /// Re-dispatch one consumed, still-held press after the timer expires.
    pub fn dispatch_repeat(
        &mut self,
        frontend: &mut InputMethodState,
        xkb_mods_depressed: u32,
        now: Instant,
    ) -> RepeatOutcome {
        let Some(key) = self.repeat_key.clone() else {
            return RepeatOutcome::Stopped;
        };
        // Check ownership and modifiers again at expiration. The platform's
        // physical hold observation catches a queued or cross-boundary release
        // even if the router has not yet processed it.
        let current_mods = Modifiers(xkb_mods_depressed);
        if !self.repeat_guard_allows(key.keycode, current_mods) {
            self.cancel_repeat();
            return RepeatOutcome::Stopped;
        }
        if !frontend.key_is_held(key.keycode) {
            self.cancel_repeat();
            return RepeatOutcome::Stopped;
        }
        if let Some(handled) = self.try_host_selection(&key, frontend, xkb_mods_depressed, now) {
            if handled {
                return RepeatOutcome::Consumed;
            }
        } else if self.dispatch_key_repeat(&key, xkb_mods_depressed) {
            return RepeatOutcome::Consumed;
        }
        self.cancel_repeat();
        RepeatOutcome::Stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard_policy::{KEY_CAPITAL_V, KEY_V};

    impl KeyboardRouter {
        /// Test-only constructor that bypasses runtime setup. The
        /// provided context pointer is stored but not dereferenced by the
        /// lifecycle helpers under test.
        pub(crate) fn new_for_test() -> Self {
            Self {
                ctx: None,
                pending_output: PendingEngineOutput::default(),
                repeat_key: None,
                repeat_arm_mods: Modifiers::NONE,
                physical_modifiers: Modifiers::NONE,
                modifiers_acquired: false,
                pressed_keys: BTreeSet::new(),
                shortcut_saw_non_modifier: false,
                chord_full_set_held: false,
                shortcut_already_triggered: false,
                shortcut_fired: false,
                engine_tracked_mods: Modifiers::NONE,
                preedit_tracking: PreeditTracking::new(),
                pending_commit_flush: None,
                preedit_coalescer: PreeditCoalescer::default(),
                voice_ptt_keycode: None,
                voice_ptt_pressed: false,
                voice_ptt_released: false,
            }
        }
    }

    #[test]
    fn page_boundary_selection_targets_page_edges() {
        assert_eq!(page_boundary_selected_index(KEY_PAGE_UP, 5), Some(4));
        assert_eq!(page_boundary_selected_index(KEY_PAGE_DOWN, 5), Some(0));
        assert_eq!(page_boundary_selected_index(KEY_PAGE_UP, 0), None);
        assert_eq!(page_boundary_selected_index(KEY_V, 5), None);
    }

    #[test]
    fn untracked_lone_shift_release_is_not_suppressed() {
        assert!(!should_suppress_untracked_modifier_release(
            Modifiers::SHIFT,
            Modifiers::SHIFT,
        ));
        assert!(should_suppress_untracked_modifier_release(
            Modifiers::SHIFT,
            Modifiers(Modifiers::SHIFT.0 | Modifiers::CTRL.0),
        ));
        assert!(should_suppress_untracked_modifier_release(
            Modifiers::CTRL,
            Modifiers::CTRL,
        ));
    }

    #[test]
    fn host_selection_only_intercepts_plain_keys() {
        assert!(host_selection_plain_key(Modifiers::NONE));
        assert!(!host_selection_plain_key(Modifiers::SHIFT));
        assert!(!host_selection_plain_key(Modifiers::CTRL));
        assert!(!host_selection_plain_key(Modifiers::ALT));
        assert!(!host_selection_plain_key(Modifiers::SUPER));
    }

    #[test]
    fn commit_candidate_falls_back_only_when_no_output_was_produced() {
        assert!(commit_candidate_should_fallback(
            &Err(typio_runtime::core::engine::EngineError::NotFound),
            false,
        ));
        assert!(!commit_candidate_should_fallback(
            &Err(typio_runtime::core::engine::EngineError::NotFound),
            true,
        ));
        assert!(commit_candidate_should_fallback(
            &Err(typio_runtime::core::engine::EngineError::Transport(
                "failed".into()
            )),
            false,
        ));
        assert!(!commit_candidate_should_fallback(
            &Err(typio_runtime::core::engine::EngineError::Transport(
                "failed".into()
            )),
            true,
        ));
        assert!(!commit_candidate_should_fallback(&Ok(()), false));
    }

    #[test]
    fn owned_commit_event_stages_router_local_output() {
        let mut left = PendingEngineOutput::default();
        let mut right = PendingEngineOutput::default();

        left.absorb(typio_runtime::input_context::ContextEvent::Commit(
            "hello".into(),
        ));

        assert_eq!(left.commit.as_deref(), Some("hello"));
        assert!(right.commit.is_none());

        right.absorb(typio_runtime::input_context::ContextEvent::Commit(
            "hello".into(),
        ));

        assert_eq!(left.commit.as_deref(), Some("hello"));
        assert_eq!(right.commit.as_deref(), Some("hello"));
    }

    fn repeat_test_key() -> DecodedKeyEvent {
        DecodedKeyEvent {
            keycode: 17,
            xkb_keycode: 25,
            keysym: 0x77,
            unicode: "w".into(),
            state: 1,
            time: 0,
        }
    }

    #[test]
    fn focus_boundary_clears_press_ownership_and_repeat() {
        let mut router = KeyboardRouter::new_for_test();
        router.on_press(17);
        router.on_consumed(repeat_test_key(), Modifiers::NONE);
        assert!(router.repeat_guard_allows(17, Modifiers::NONE));
        router.fence_key_routing();
        assert!(!router.owns_press(17));
        assert!(!router.repeat_guard_allows(17, Modifiers::NONE));
        assert!(router.repeat_key.is_none());
    }

    #[test]
    fn unrelated_release_does_not_stop_repeat() {
        let mut router = KeyboardRouter::new_for_test();
        router.on_press(17);
        router.on_press(30);
        router.on_consumed(repeat_test_key(), Modifiers::NONE);
        let mut release = repeat_test_key();
        release.state = 0;
        release.keycode = 30;
        assert!(!router.on_release(&release));
        assert!(router.repeat_key.is_some());
        release.keycode = 17;
        assert!(router.on_release(&release));
        assert!(router.repeat_key.is_none());
    }

    #[test]
    fn transient_blocking_modifier_cancels_repeat() {
        let mut router = KeyboardRouter::new_for_test();
        router.on_press(17);
        router.on_consumed(repeat_test_key(), Modifiers::NONE);
        assert!(!router.observe_modifiers(Modifiers::SHIFT));
        assert!(router.observe_modifiers(Modifiers::CTRL));
        router.observe_modifiers(Modifiers::NONE);
        assert!(router.repeat_key.is_none());
    }

    #[test]
    fn unowned_modifier_release_clears_baseline_without_completing_chord() {
        let mut router = KeyboardRouter::new_for_test();
        router.acquire_modifiers_if_needed(Modifiers::CTRL.0);
        let mut release = repeat_test_key();
        release.keysym = crate::keyboard_policy::KEY_CONTROL_L;
        release.state = 0;
        router.on_unowned_release(&release);
        assert!(!router.physical_modifiers.intersects(Modifiers::CTRL));
        assert!(!router.take_switch_chord_fired());
    }

    #[test]
    fn is_focused_handles_null_ctx_gracefully() {
        let router = KeyboardRouter::new_for_test();
        assert!(!router.is_focused());
    }

    #[test]
    fn preedit_tracking_starts_empty_and_resets() {
        let mut router = KeyboardRouter::new_for_test();
        router.preedit_tracking.last_text = Some("ni".to_string());
        router.preedit_tracking.last_cursor = 2;
        router.preedit_tracking_reset();
        assert_eq!(router.preedit_tracking.last_text, None);
        assert_eq!(router.preedit_tracking.last_cursor, -1);
    }

    #[test]
    fn preedit_dedup_classifies_arrow_vs_text_change() {
        // Simulate the canonical arrow-navigation case: same preedit
        // text and cursor, only the selected candidate index differs.
        // The plan must be SyncPanelOnly so `drain_composition` skips
        // the `set_preedit_string + commit` Wayland round-trip.
        let plan = text_ui_plan_update(Some("nihao"), 5, Some("nihao"), 5);
        assert_eq!(plan, TextUiPlan::SyncPanelOnly);

        // A real preedit edit transitions to SyncPreeditAndPanel.
        let plan = text_ui_plan_update(Some("ni"), 2, Some("nih"), 3);
        assert_eq!(plan, TextUiPlan::SyncPreeditAndPanel);

        // Cursor-only move inside the same preedit also resends.
        let plan = text_ui_plan_update(Some("ni"), 1, Some("ni"), 2);
        assert_eq!(plan, TextUiPlan::SyncPreeditAndPanel);
    }

    #[test]
    fn switch_chord_fires_on_clean_ctrl_shift_release() {
        let mut router = KeyboardRouter::new_for_test();
        // Press Ctrl, then Shift: the completing press must NOT fire.
        assert!(!router.track_switch_modifier(Modifiers::CTRL, true));
        assert!(!router.track_switch_modifier(Modifiers::SHIFT, true));
        assert!(router.chord_full_set_held);
        // Releasing the first chord modifier completes the switch.
        assert!(router.track_switch_modifier(Modifiers::SHIFT, false));
        // The second release must not fire again in the same gesture.
        assert!(!router.track_switch_modifier(Modifiers::CTRL, false));
        // Gesture fully ended: state reset.
        assert!(!router.chord_full_set_held);
        assert!(!router.shortcut_already_triggered);
    }

    #[test]
    fn switch_chord_suppressed_when_non_modifier_joins() {
        // The Ctrl+Shift+V regression: a non-modifier key during the
        // gesture must cancel the switch even if it lifts before the
        // modifiers do (the latch stays set until the gesture ends).
        let mut router = KeyboardRouter::new_for_test();
        router.track_switch_modifier(Modifiers::CTRL, true);
        router.track_switch_modifier(Modifiers::SHIFT, true);
        // V down then up (dispatch_key sets the latch on a non-mod press).
        router.shortcut_saw_non_modifier = true;
        // Releasing the modifiers must NOT switch.
        assert!(!router.track_switch_modifier(Modifiers::SHIFT, false));
        assert!(!router.track_switch_modifier(Modifiers::CTRL, false));
    }

    #[test]
    fn super_v_press_and_release_are_voice_ptt_shortcut() {
        let mut router = KeyboardRouter::new_for_test();
        let press = DecodedKeyEvent {
            keycode: 47,
            xkb_keycode: 55,
            keysym: KEY_V,
            unicode: "v".to_string(),
            state: WL_KEYBOARD_KEY_STATE_PRESSED,
            time: 10,
        };
        let release = DecodedKeyEvent {
            state: WL_KEYBOARD_KEY_STATE_RELEASED,
            time: 20,
            ..press.clone()
        };

        assert!(router.dispatch_key(&press, Modifiers::SUPER.0));
        assert!(router.take_voice_ptt_pressed());
        assert!(!router.take_voice_ptt_pressed());

        assert!(router.dispatch_key(&release, Modifiers::NONE.0));
        assert!(router.take_voice_ptt_released());
        assert!(!router.take_voice_ptt_released());
    }

    #[test]
    fn super_capital_v_is_voice_ptt_shortcut() {
        let mut router = KeyboardRouter::new_for_test();
        let press = DecodedKeyEvent {
            keycode: 47,
            xkb_keycode: 55,
            keysym: KEY_CAPITAL_V,
            unicode: "V".to_string(),
            state: WL_KEYBOARD_KEY_STATE_PRESSED,
            time: 10,
        };

        assert!(router.dispatch_key(&press, Modifiers::SUPER.0));
        assert!(router.take_voice_ptt_pressed());
    }

    #[test]
    fn ctrl_super_v_does_not_trigger_voice_ptt() {
        let mut router = KeyboardRouter::new_for_test();
        let press = DecodedKeyEvent {
            keycode: 47,
            xkb_keycode: 55,
            keysym: KEY_V,
            unicode: "v".to_string(),
            state: WL_KEYBOARD_KEY_STATE_PRESSED,
            time: 10,
        };

        let mods = Modifiers::SUPER.0 | Modifiers::CTRL.0;
        assert!(!router.dispatch_key(&press, mods));
        assert!(!router.take_voice_ptt_pressed());
    }

    #[test]
    fn single_modifier_release_does_not_switch() {
        let mut router = KeyboardRouter::new_for_test();
        // Only Ctrl is ever held: the full set is never reached.
        assert!(!router.track_switch_modifier(Modifiers::CTRL, true));
        assert!(!router.track_switch_modifier(Modifiers::CTRL, false));
    }

    #[test]
    fn switch_chord_rearms_after_prior_typing() {
        // A taint left over from earlier typing must not block the next
        // chord: starting a fresh gesture (first chord modifier down from
        // clean) clears the latch.
        let mut router = KeyboardRouter::new_for_test();
        router.shortcut_saw_non_modifier = true; // stale taint
        router.track_switch_modifier(Modifiers::CTRL, true);
        router.track_switch_modifier(Modifiers::SHIFT, true);
        assert!(router.track_switch_modifier(Modifiers::SHIFT, false));
    }

    #[test]
    fn reset_key_routing_clears_switch_chord_state() {
        let mut router = KeyboardRouter::new_for_test();
        router.track_switch_modifier(Modifiers::CTRL, true);
        router.track_switch_modifier(Modifiers::SHIFT, true);
        router.shortcut_saw_non_modifier = true;
        assert!(router.chord_full_set_held);

        router.reset_key_routing();

        assert!(!router.shortcut_saw_non_modifier);
        assert!(!router.chord_full_set_held);
        assert!(!router.shortcut_already_triggered);
    }

    // ── engine modifier-mask regression tests ───────────────────────────
    //
    // These cover the bug where Super+Shift (or any chord involving a
    // blocking modifier) was misreported to the engine as a lone Shift,
    // triggering an unwanted mode toggle. They drive the real
    // `dispatch_key` path (seeding + track_switch_modifier + mask
    // computation) and read back the exact mask the engine would receive
    // via `engine_modifier_mask`, without needing a live runtime context.

    use crate::keyboard_policy::{KEY_ALT_L, KEY_CONTROL_L, KEY_SHIFT_L, KEY_SUPER_L};

    /// Build a modifier-key press/release event for the given keysym.
    fn mod_key(keysym: u32, pressed: bool, time: u32) -> DecodedKeyEvent {
        DecodedKeyEvent {
            keycode: 0,
            xkb_keycode: 0,
            keysym,
            unicode: "\0".to_string(),
            state: if pressed { 1 } else { 0 },
            time,
        }
    }

    #[test]
    fn super_shift_release_carries_super_bit_not_lone_shift() {
        // Regression: the canonical failing case. Super is held, then
        // Shift is pressed and released. The Shift *release* must reach
        // the engine still carrying the Super bit, so the engine sees a
        // chord (Super+Shift) and does NOT treat it as a lone-Shift
        // mode toggle.
        let mut router = KeyboardRouter::new_for_test();

        // Press Super. xkb snapshot reports Super. Mask must include it.
        router.dispatch_key(&mod_key(KEY_SUPER_L, true, 1), Modifiers::SUPER.0);
        assert!(
            router
                .engine_modifier_mask(&mod_key(KEY_SUPER_L, true, 1), Modifiers::SUPER.0)
                .intersects(Modifiers::SUPER)
        );

        // Press Shift while Super is held.
        router.dispatch_key(
            &mod_key(KEY_SHIFT_L, true, 2),
            Modifiers::SUPER.0 | Modifiers::SHIFT.0,
        );

        // Release Shift. Simulate the per-step xkb snapshot having
        // already dropped Shift (Modifiers event racing ahead of the
        // Key event) — mods_depressed = Super only. The engine must
        // STILL see Super on this Shift release.
        let mask = router.engine_modifier_mask(&mod_key(KEY_SHIFT_L, false, 3), Modifiers::SUPER.0);
        assert!(
            mask.intersects(Modifiers::SUPER),
            "Super must survive onto the Shift release; got mask {:?}",
            mask
        );
        // And the Shift bit must be cleared (this *is* the Shift release).
        assert!(
            !mask.intersects(Modifiers::SHIFT),
            "released modifier's own bit must clear; got mask {:?}",
            mask
        );
    }

    #[test]
    fn super_shift_release_carries_super_even_when_xkb_snapshot_is_empty() {
        // Harder variant: the per-step xkb snapshot reports NO modifiers
        // at all at release time (worst-case stale snapshot). Because we
        // seed physical state and track per-key, Super must still be
        // present on the Shift release.
        let mut router = KeyboardRouter::new_for_test();

        router.dispatch_key(&mod_key(KEY_SUPER_L, true, 1), Modifiers::SUPER.0);
        router.dispatch_key(
            &mod_key(KEY_SHIFT_L, true, 2),
            Modifiers::SUPER.0 | Modifiers::SHIFT.0,
        );

        let mask = router.engine_modifier_mask(&mod_key(KEY_SHIFT_L, false, 3), Modifiers::NONE.0);
        assert!(
            mask.intersects(Modifiers::SUPER),
            "Super must survive even with a stale-empty xkb snapshot; got {:?}",
            mask
        );
    }

    #[test]
    fn lone_shift_release_has_no_blocking_bits() {
        // Positive control: a genuinely lone Shift press/release (no
        // sibling modifier) must reach the engine with an empty blocking
        // mask. This is the gesture engines DO treat as a mode toggle,
        // and the fix must not suppress it.
        let mut router = KeyboardRouter::new_for_test();

        router.dispatch_key(&mod_key(KEY_SHIFT_L, true, 1), Modifiers::SHIFT.0);

        // Release with a fully-stale xkb snapshot.
        let mask = router.engine_modifier_mask(&mod_key(KEY_SHIFT_L, false, 2), Modifiers::NONE.0);
        assert!(
            !mask.intersects(Modifiers::SUPER),
            "no Super should be present on a lone Shift release; got {:?}",
            mask
        );
        assert!(
            !mask.intersects(Modifiers::SHIFT),
            "Shift's own bit must clear on release; got {:?}",
            mask
        );
        assert!(
            !mask.intersects(Modifiers::CTRL) && !mask.intersects(Modifiers::ALT),
            "no other blocking bits expected; got {:?}",
            mask
        );
    }

    #[test]
    fn alt_shift_release_carries_alt_bit() {
        // The fix generalises to any blocking sibling, not just Super.
        let mut router = KeyboardRouter::new_for_test();

        router.dispatch_key(&mod_key(KEY_ALT_L, true, 1), Modifiers::ALT.0);
        router.dispatch_key(
            &mod_key(KEY_SHIFT_L, true, 2),
            Modifiers::ALT.0 | Modifiers::SHIFT.0,
        );

        let mask = router.engine_modifier_mask(&mod_key(KEY_SHIFT_L, false, 3), Modifiers::NONE.0);
        assert!(
            mask.intersects(Modifiers::ALT),
            "Alt must survive onto the Shift release; got {:?}",
            mask
        );
    }

    #[test]
    fn ctrl_shift_chord_still_fires_switch() {
        // The existing Ctrl+Shift engine-switch chord must keep working
        // after the mask fix — the chord detection is independent of the
        // mask reported to the engine.
        let mut router = KeyboardRouter::new_for_test();

        // Ctrl down, Shift down, Shift up: chord must fire exactly once.
        assert!(!router.take_switch_chord_fired());
        router.dispatch_key(&mod_key(KEY_CONTROL_L, true, 1), Modifiers::CTRL.0);
        assert!(!router.take_switch_chord_fired());
        router.dispatch_key(
            &mod_key(KEY_SHIFT_L, true, 2),
            Modifiers::CTRL.0 | Modifiers::SHIFT.0,
        );
        assert!(!router.take_switch_chord_fired());
        router.dispatch_key(&mod_key(KEY_SHIFT_L, false, 3), Modifiers::CTRL.0);
        assert!(
            router.take_switch_chord_fired(),
            "Ctrl+Shift chord must still fire"
        );
        assert!(!router.take_switch_chord_fired(), "fires only once");
    }

    #[test]
    fn super_shift_does_not_fire_ctrl_shift_chord() {
        // Super+Shift must NOT be mistaken for the Ctrl+Shift switch.
        let mut router = KeyboardRouter::new_for_test();

        router.dispatch_key(&mod_key(KEY_SUPER_L, true, 1), Modifiers::SUPER.0);
        router.dispatch_key(
            &mod_key(KEY_SHIFT_L, true, 2),
            Modifiers::SUPER.0 | Modifiers::SHIFT.0,
        );
        router.dispatch_key(&mod_key(KEY_SHIFT_L, false, 3), Modifiers::SUPER.0);
        assert!(
            !router.take_switch_chord_fired(),
            "Super+Shift must not fire the Ctrl+Shift switch chord"
        );
    }

    #[test]
    fn grab_handoff_does_not_leak_held_modifier() {
        // A modifier held across a focus/generation boundary must not
        // leak into the new generation: reset_key_routing resets the
        // baseline, and the first event of the new generation re-seeds
        // it from xkb rather than trusting stale physical state.
        let mut router = KeyboardRouter::new_for_test();

        // Hold Super in the old generation.
        router.dispatch_key(&mod_key(KEY_SUPER_L, true, 1), Modifiers::SUPER.0);
        assert!(router.physical_modifiers.intersects(Modifiers::SUPER));

        // Generation boundary (focus change / grab handoff).
        router.reset_key_routing();
        assert!(!router.physical_modifiers.intersects(Modifiers::SUPER));
        assert!(!router.modifiers_acquired);

        // In the new generation Super is NOT held (the new surface
        // doesn't have it). The first event must seed from xkb (empty),
        // not from the stale pre-handoff physical state.
        router.dispatch_key(&mod_key(KEY_SHIFT_L, true, 2), Modifiers::SHIFT.0);
        let mask = router.engine_modifier_mask(&mod_key(KEY_SHIFT_L, true, 2), Modifiers::SHIFT.0);
        assert!(
            !mask.intersects(Modifiers::SUPER),
            "stale Super must not leak across generation boundary; got {:?}",
            mask
        );
    }

    #[test]
    fn modifier_held_at_grab_start_is_seeded() {
        // If a modifier was already down when the grab started (so no
        // press event arrives for it), the first event of the generation
        // must seed it from the xkb snapshot, so a subsequent sibling
        // release still carries it.
        let mut router = KeyboardRouter::new_for_test();

        // No Super press event; it was already held. First event is the
        // Shift press, whose xkb snapshot reports Super|Shift.
        router.dispatch_key(
            &mod_key(KEY_SHIFT_L, true, 1),
            Modifiers::SUPER.0 | Modifiers::SHIFT.0,
        );

        // Shift release with a stale snapshot still carries Super.
        let mask = router.engine_modifier_mask(&mod_key(KEY_SHIFT_L, false, 2), Modifiers::NONE.0);
        assert!(
            mask.intersects(Modifiers::SUPER),
            "Super held at grab start must be seeded; got {:?}",
            mask
        );
    }
}
