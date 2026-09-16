//! Keyboard input and repeat driver for one reactor step.
//!
//! Ordering-sensitive engine, preedit, virtual-keyboard, shortcut, and repeat
//! effects stay together here. The outer event loop supplies readiness and
//! then continues with Panel/status/auxiliary drivers.

use std::time::Instant;

use crate::input_method::KeyboardInput;
use typio_host_types::Modifiers;

use crate::keyboard::router::RepeatOutcome;

use super::{App, DaemonEvent, arm_repeat, tray::cycle_active_language};

impl App {
    pub(super) fn drive_pending_keys(&mut self, now: Instant) {
        // Drain one ordered keyboard stream, including boundaries and modifiers.
        // Press ownership is checked before engine or virtual-keyboard routing.
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

            let pending_input = state.take_pending_input();
            let pending_key_count = pending_input.len();
            let mut host_nav_updates = 0usize;
            for input in pending_input {
                let Some(input) = state.prepare_input(input) else {
                    continue;
                };
                let (key, mods) = match input {
                    KeyboardInput::Boundary => {
                        router.fence_key_routing();
                        let _ = timer.stop();
                        continue;
                    }
                    KeyboardInput::Modifiers(sample) => {
                        if router.observe_modifiers(sample.effective) {
                            let _ = timer.stop();
                        }
                        continue;
                    }
                    KeyboardInput::Key { key, modifiers, .. } => (key, modifiers.0),
                };
                let compositor_info = state.compositor_repeat_info;
                if key.state == 1 {
                    router.on_press(key.keycode);
                    router.cancel_repeat();
                    let _ = timer.stop();
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
                    // modifier-only events, and filtered-out keys do not.
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
                        if state.key_repeats(key.keycode) {
                            router.on_consumed(key.clone(), Modifiers(mods));
                            arm_repeat(timer, compositor_info, mods);
                        }
                    } else {
                        // Forwarded presses use the application's native repeat.
                        // Only consumed presses use the host's repeat timer.
                        state.forward_key(key.time, key.keycode, key.state);
                    }
                } else {
                    let press_was_forwarded = state.key_is_forwarded(key.keycode);
                    // No engine release or shortcut completion may cross an
                    // epoch. The transport still pairs any forwarded press.
                    if !router.owns_press(key.keycode) {
                        router.on_unowned_release(&key);
                        state.forward_key(key.time, key.keycode, 0);
                        continue;
                    }
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
                    if router.on_release(&key) {
                        let _ = timer.stop();
                    }
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
                // Input processing may stop or rearm the timer after poll
                // reported readiness. That readiness then has no expiration.
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    return;
                }
                tracing::warn!(
                    target: "typio.input.repeat",
                    %error,
                    "failed to consume repeat timer expiration"
                );
                return;
            }
            let mods = state.effective_modifiers().0;
            // A repeat chain belongs to the focused field the key was
            // pressed in. If the input method has gone inactive since the
            // timer was armed, end the chain instead of injecting synthetic
            // keys into whatever surface may have focus now.
            if !state.is_active() {
                router.cancel_repeat();
                let _ = timer.stop();
                return;
            }
            let seq_before_repeat = state.composition.composition_seq;
            let outcome = router.dispatch_repeat(state, mods, now);
            match outcome {
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
