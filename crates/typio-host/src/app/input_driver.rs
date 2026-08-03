//! Keyboard input and repeat driver for one reactor step.
//!
//! Ordering-sensitive engine, preedit, virtual-keyboard, shortcut, and repeat
//! effects stay together here. The outer event loop supplies readiness and
//! then continues with Panel/status/auxiliary drivers.

use std::time::Instant;

use crate::keyboard::router::RepeatOutcome;

use super::{App, DaemonEvent, arm_repeat, tray::cycle_active_language};

impl App {
    pub(super) fn drive_pending_keys(&mut self, now: Instant) {
        // Drain engine output and process all pending key events.
        //    Draining the whole queue (not just one event) is what
        //    prevents the "stuck backspace" symptom: if a release and
        //    a subsequent press arrive in the same Wayland dispatch
        //    batch, both must reach the router in order — losing the
        //    release leaves the repeat timer armed forever.
        let mut voice_status_to_show: Option<String> = None;
        {
            let frontend = self.frontend.as_mut().unwrap();
            let state = frontend.state_mut();
            let router = self.router.as_mut().unwrap();
            let timer = self.repeat_timer.as_mut().unwrap();

            router.drain_commit();
            router.drain_composition(state, now);
            // Real commit text must precede the next key. Pure preedit waits
            // until the queued key batch has converged below, even when its
            // deadline is already due, so an already-readable key can replace
            // it without producing another same-serial intermediate commit.
            router.flush_pending_text_if_commit(state);

            let pending_keys = state.take_pending_keys();
            if !pending_keys.is_empty() {
                tracing::debug!(
                    target: "typio.input.queue",
                    pending_key_count = pending_keys.len(),
                    "drain pending keys"
                );
            }
            let pending_key_count = pending_keys.len();
            let mut host_nav_updates = 0usize;
            for key in pending_keys {
                // Snapshot the values we need before any mutable borrows
                // below — both are cheap `Copy` reads.
                let mods = state.mods_depressed;
                let compositor_info = state.compositor_repeat_info;
                if key.state == 1 {
                    // Try host-managed selection (ADR-0012) first.
                    // Engines that opt in get their navigation/commit
                    // keys handled locally without a synchronous FFI
                    // round-trip; engines that don't opt in see no
                    // behaviour change.
                    let seq_before_host_selection = state.composition.composition_seq;
                    let consumed = match router.try_host_selection(&key, state, mods, now) {
                        Some(handled) => {
                            if handled
                                && state.composition.composition_seq != seq_before_host_selection
                            {
                                host_nav_updates += 1;
                            }
                            handled
                        }
                        None => router.dispatch_key(&key, mods),
                    };
                    // Any key that reached the engine (consumed or
                    // forwarded) counts as "user activity" for the
                    // indicator's acknowledged-recency gate. Releases,
                    // modifier-only events, and filtered-out keys do
                    // not (mirrors the C `record_key_activity` caller
                    // in keyboard.c).
                    if let Some(indicator) = self.indicator.as_mut() {
                        indicator.record_key_activity(now);
                    }
                    if router.take_switch_chord_fired() {
                        // Ctrl+Shift (default) just completed. Cycle
                        // to the next language (reusing the engine last
                        // used for it); with <2 languages the cycle
                        // falls back to engine cycling. Suppresses
                        // forwarding of the modifier press itself.
                        tracing::debug!(target: "typio.indicator", "Ctrl+Shift language-switch chord fired");
                        if let Some(instance) = self.instance.as_ref() {
                            cycle_active_language(instance);
                        }
                        let _ = self.event_tx.send(DaemonEvent::StateRefresh);
                    } else if router.take_voice_ptt_pressed() {
                        tracing::debug!(target: "typio.voice", "Super+V push-to-talk pressed");
                        let _ = timer.stop();
                        // On success the session fires a synchronous
                        // `Recording` state change that drives the sticky
                        // "listening…" banner; only surface a transient
                        // banner here for the failure / unavailable paths.
                        voice_status_to_show = match self.voice.as_ref() {
                            Some(voice) if voice.is_available() => {
                                if voice.start() {
                                    None
                                } else {
                                    Some("Voice: could not start audio capture".to_string())
                                }
                            }
                            Some(voice) => Some(format!("Voice: {}", voice.unavail_reason())),
                            None => Some("Voice: unavailable".to_string()),
                        };
                    } else if consumed {
                        // Engine consumed the key. Drain any output it
                        // produced, then arm the repeat timer in engine
                        // mode so the held key re-dispatches with
                        // `is_repeat: true` (e.g. backspace deleting a
                        // long preedit one character per repeat expiration).
                        router.drain_commit();
                        router.drain_composition(state, now);
                        router.flush_pending_text_if_commit(state);
                        router.on_consumed(key.clone());
                        arm_repeat(timer, compositor_info, mods);
                    } else {
                        // Engine declined the key; forward it to the
                        // focused app and arm the timer in forward mode
                        // so the main loop synthesises repeats.
                        state.forward_key(key.time, key.keycode, key.state);
                        router.on_forward(key.clone());
                        arm_repeat(timer, compositor_info, mods);
                    }
                } else {
                    // Soft-pause already synthesized a virtual-keyboard
                    // release for this keycode; swallow the physical one.
                    if router.release_is_pending(key.keycode) {
                        state.clear_synthetic_release(key.keycode);
                        router.on_release(&key);
                        let _ = timer.stop();
                        continue;
                    }
                    // Forward release events to the engine so
                    // engines that need them (e.g. Rime schema
                    // switching on a lone Shift release) can
                    // complete gesture detection. Host-managed
                    // selection releases are swallowed by
                    // `try_host_selection` so the engine never sees
                    // an unpaired release for a press the host
                    // intercepted — unless that press was earlier
                    // forwarded to the app, in which case the
                    // release must still pair through the virtual
                    // keyboard (stuck-Space class of bugs).
                    let press_was_forwarded = router.press_was_forwarded(key.keycode);
                    let consumed = match router.try_host_selection(&key, state, mods, now) {
                        Some(handled) => handled,
                        None => router.dispatch_key(&key, mods),
                    };
                    if router.take_voice_ptt_released() {
                        tracing::debug!(target: "typio.voice", "Super+V push-to-talk released");
                        if let Some(voice) = self.voice.as_ref() {
                            // The session fires a synchronous `Processing`
                            // state change that drives the sticky
                            // "transcribing…" banner.
                            voice.stop();
                        }
                    } else {
                        if consumed {
                            router.drain_commit();
                            router.drain_composition(state, now);
                            router.flush_pending_text_if_commit(state);
                        }
                        // Symmetric release: a press that went to the
                        // app must deliver a matching release, even when
                        // the engine consumes the release event.
                        if press_was_forwarded || !consumed {
                            state.forward_key(key.time, key.keycode, key.state);
                        }
                    }
                    state.clear_synthetic_release(key.keycode);
                    router.on_release(&key);
                    let _ = timer.stop();
                }
            }
            router.flush_pending_text_if_due(state, now);
            if tracing::enabled!(target: "typio.input.perf", tracing::Level::TRACE)
                && (pending_key_count > 1 || host_nav_updates > 0)
            {
                tracing::trace!(
                    target: "typio.input.perf",
                    pending_key_count,
                    host_nav_updates,
                    coalesced_host_nav_updates = host_nav_updates.saturating_sub(1),
                    final_composition_seq = state.composition.composition_seq,
                    "drained pending key batch"
                );
            }
        }
        if let Some(label) = voice_status_to_show {
            // Press/release only surface transient feedback here (errors,
            // unavailable); the in-progress banners are driven by the
            // session state machine in `handle_voice_outcome`.
            self.show_voice_transient(label);
        }
    }

    pub(super) fn drive_repeat(&mut self, repeat_ready: bool, now: Instant) {
        // Dispatch one ready repeat timer before Panel presentation.
        //
        // Handle repeats before candidate-panel flush so host-managed
        // navigation produced by a repeat can repaint in this reactor step.
        // Previously repeats ran after panel flush, adding one full loop
        // of latency to held Up/Down candidate movement.
        if repeat_ready {
            let frontend = self.frontend.as_mut().unwrap();
            let state = frontend.state_mut();
            let router = self.router.as_mut().unwrap();
            let timer = self.repeat_timer.as_mut().unwrap();
            if let Err(error) = timer.consume_expiration() {
                tracing::warn!(
                    target: "typio.input.repeat",
                    %error,
                    "failed to consume repeat timer expiration"
                );
                return;
            }
            let mods = state.mods_depressed;
            let seq_before_repeat = state.composition.composition_seq;
            let outcome = router.dispatch_repeat(state, mods, now);
            match outcome {
                RepeatOutcome::Forwarded => {}
                RepeatOutcome::Consumed => {
                    router.drain_commit();
                    router.drain_composition(state, now);
                    router.flush_pending_text_if_commit(state);
                }
                RepeatOutcome::Stopped => {
                    let _ = timer.stop();
                }
            }
            router.flush_pending_text_if_due(state, now);
            let seq_after_repeat = state.composition.composition_seq;
            if tracing::enabled!(target: "typio.input.perf", tracing::Level::TRACE) {
                tracing::trace!(
                    target: "typio.input.perf",
                    ?outcome,
                    seq_before_repeat,
                    seq_after_repeat,
                    changed_selection = seq_after_repeat != seq_before_repeat,
                    "repeat dispatched before panel flush"
                );
            }
        }
    }
}
