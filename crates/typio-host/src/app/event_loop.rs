//! The Wayland event-loop driver.
//!
//! `App::run_with_wayland` is the main per-tick pipeline: flush,
//! prepare-read, dispatch, poll, read, focus-controller, key drain,
//! panel flush, repeat, config-reload. Each stage is annotated with
//! `LoopStage` so the watchdog can attribute stalls.

use std::cell::RefCell;
use std::ffi::c_void;
use std::os::fd::{AsFd, AsRawFd};
use std::rc::Rc;
use std::time::Instant;

use typio::instance::TypioInstance;

use crate::ipc_bus::IpcBus;
use crate::keyboard::router::RepeatOutcome;
use crate::panel_coordinator::UiOwner;
use crate::panel_present_gate::PresentDecision;
use crate::panel_scheduler;
use crate::session_glue::FocusTransition;
use crate::watchdog::LoopStage;

use super::{arm_repeat, tray::cycle_active_language, App, DaemonEvent};

/// What the candidate-panel flush path actually did this tick.
///
/// Replaces an ambiguous `(presented: bool, hid: bool)` pair whose
/// `(false, false)` variant conflated three structurally different
/// outcomes — "already shown, nothing to do", "present-path frame drop,
/// retry", and "no panel surface" — and let the retry guard mis-classify
/// the already-shown skip as a dropped frame, busy-looping with the
/// schedule pinned Dirty. One variant per outcome makes the settle
/// action exhaustive and the convergence invariant local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlushOutcome {
    /// A fresh buffer was attached and reached the compositor. Settle the
    /// schedule to Idle, record `composition_seq` as presented, and arm the
    /// next `wl_surface.frame` callback.
    Presented,
    /// The surface was hidden because the candidate list emptied. Clear the
    /// frame callback and invalidate the presentation record; the schedule
    /// settles to Idle (a future candidate update re-dirties it).
    Hidden,
    /// `composition_seq` was already presented in this generation. Nothing to
    /// draw, nothing to retry: settle to Idle without touching the
    /// presentation record.
    AlreadyPresented,
    /// Skipped for pacing — anchor not ready, or the present soft-gate wants
    /// to wait for a frame callback. Keep the schedule Dirty so the waking
    /// tick re-attempts with the latest coalesced candidates.
    Throttled,
    /// `draw_candidates` ran but attached no buffer (dma-buf slot busy, SHM
    /// pool exhausted, or a flux begin/submit/export failure). The frame
    /// never reached the compositor, so keep the schedule Dirty and re-arm
    /// the next tick rather than marking `composition_seq` done and losing
    /// it.
    Dropped,
    /// No panel surface exists this tick. Keep the schedule Dirty so the
    /// flush re-attempts once the surface is (re)created.
    NoPanel,
}

impl App {
    /// The Wayland main loop. Returns the daemon exit code.
    ///
    /// Stages per tick (annotated with `LoopStage` for the watchdog):
    ///   Idle → AuxIo (focus controller) → Flush → PrepareRead →
    ///   DispatchPending → Poll → ReadEvents → Repeat → PanelUpdate →
    ///   ConfigReload.
    pub(super) fn run_with_wayland(&mut self, ipc_bus: &Rc<RefCell<IpcBus>>) -> i32 {
        let wl_fd = self.frontend.as_mut().unwrap().fd();
        let uds_fd = ipc_bus.borrow().epoll_fd();
        let repeat_fd = self.repeat_timer.as_mut().unwrap().fd();
        // Re-borrow the watchdog field on every use so a long-lived immutable
        // borrow does not block the mutable borrow needed by reload_config().
        macro_rules! wd {
            () => {
                self.watchdog.as_ref().unwrap()
            };
        }

        let (inotify_fd, cfg_timer_fd) = self
            .config_watcher
            .as_ref()
            .map(|w| (w.inotify_fd(), w.timer_fd()))
            .unwrap_or((-1, -1));

        // Indicator auto-hide timer. We pull the raw fd up-front (stable
        // for the timerfd's lifetime) so we can add it to the static poll
        // set; the timer is armed/disarmed via `TimerFd::set` elsewhere.
        let indicator_fd = self
            .indicator_timer
            .as_ref()
            .map(|t| t.as_fd().as_raw_fd())
            .unwrap_or(-1);
        let voice_status_fd = self
            .voice_status_timer
            .as_ref()
            .map(|t| t.as_fd().as_raw_fd())
            .unwrap_or(-1);
        // Voice session eventfd: becomes readable when inference completes
        // (or an async model load resolves). Stable for the session
        // lifetime, so it can join the static poll set.
        let voice_session_fd = self.voice.as_ref().map(|v| v.session_fd()).unwrap_or(-1);

        let mut fds = [
            libc::pollfd {
                fd: wl_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: uds_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: repeat_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: inotify_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: cfg_timer_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: indicator_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: voice_status_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: voice_session_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        while !self.drain_events() {
            wd!().set_stage(LoopStage::Idle);
            wd!().heartbeat();

            // 1. Start-of-tick fact bookkeeping.
            {
                let frontend = self.frontend.as_mut().unwrap();
                frontend.state_mut().facts_mut().connection_alive = true;
                if let Some(ref mut rs) = self.resume_signal {
                    if !rs.tick().is_empty() {
                        frontend.state_mut().facts_mut().suspend_gap_detected = true;
                    }
                }
                if frontend.stopped() {
                    tracing::error!(target: "typio.lifecycle", "input method unavailable; exiting");
                    return 1;
                }
            }

            // 2. Flush outgoing Wayland requests, then prepare a read and
            //    dispatch any already-queued events before polling.
            wd!().set_stage(LoopStage::Flush);
            {
                let frontend = self.frontend.as_ref().unwrap();
                if let Err(e) = frontend.flush() {
                    tracing::error!(target: "typio.wayland.io", error = %e, "flush failed");
                    return 1;
                }
            }
            wd!().set_stage(LoopStage::PrepareRead);
            let read_guard = {
                let frontend = self.frontend.as_mut().unwrap();
                match frontend.prepare_read_loop() {
                    Ok(guard) => Some(guard),
                    Err(e) => {
                        tracing::error!(target: "typio.wayland.io", error = %e, "prepare_read failed");
                        return 1;
                    }
                }
            };
            wd!().set_stage(LoopStage::DispatchPending);
            ipc_bus.borrow_mut().dispatch();

            for slot in fds.iter_mut() {
                slot.revents = 0;
            }

            // 3. Poll. Let panel/anchor/status deadlines shorten the timeout.
            wd!().set_stage(LoopStage::Poll);
            wd!().heartbeat();
            let timeout_ms = {
                let frontend = self.frontend.as_ref().unwrap();
                let state = frontend.state();
                let router = self.router.as_ref().unwrap();
                let flushable = panel_scheduler::should_flush(
                    state.panel_schedule_state,
                    router.is_focused(),
                    !router.ctx().is_null(),
                    router.is_focused(),
                );
                let mut timeout_ms = -1;
                let now = Instant::now();
                let mut reduce_timeout = |remaining: i32| {
                    if timeout_ms < 0 || remaining < timeout_ms {
                        timeout_ms = remaining;
                    }
                };
                if let Some(remaining) = state.panel_coord.anchor_deadline_remaining_ms(now) {
                    reduce_timeout(remaining as i32);
                }
                if flushable && !state.composition.candidates.is_empty() {
                    if let Some(remaining) = state.panel_present_wait_remaining_ms(now) {
                        reduce_timeout(remaining);
                    }
                }
                if let Some(indicator_remaining) = self.indicator_hide_remaining_ms(now) {
                    reduce_timeout(indicator_remaining);
                }
                if let Some(voice_remaining) = self.voice_status_hide_remaining_ms(now) {
                    reduce_timeout(voice_remaining);
                }
                if let Some(remaining) = state.wayland_pending.min_deadline_ms(now) {
                    reduce_timeout(remaining);
                }
                timeout_ms
            };
            let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout_ms) };
            if rc < 0 {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                tracing::error!(target: "typio.lifecycle", error = %e, "poll failed");
                return 1;
            }

            // 4. Read and dispatch new Wayland events, or cancel the prepared read.
            wd!().set_stage(LoopStage::ReadEvents);
            if fds[0].revents & libc::POLLIN != 0 {
                let frontend = self.frontend.as_mut().unwrap();
                if let Some(guard) = read_guard {
                    let timing_enabled =
                        tracing::enabled!(target: "typio.wayland.io", tracing::Level::DEBUG);
                    let started = timing_enabled.then(Instant::now);
                    if let Err(e) = frontend.read_and_dispatch(guard) {
                        tracing::error!(target: "typio.wayland.io", error = %e, "read_and_dispatch failed");
                        return 1;
                    }
                    if let Some(started) = started {
                        tracing::debug!(
                            target: "typio.wayland.io",
                            elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                            "read_and_dispatch"
                        );
                    }
                }
            } else if fds[0].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                tracing::error!(target: "typio.wayland.io", "display disconnected");
                return 1;
            }
            // If POLLIN was not set, `read_guard` is dropped here and cancels the read.

            // 4b. Diagnose compositor requests that never got a response
            //     (commit→done, grab→keymap, probe→rect). Emits one warn per
            //     stalled episode; see `wayland_pending`.
            {
                let frontend = self.frontend.as_mut().unwrap();
                frontend
                    .state_mut()
                    .wayland_pending
                    .check_timeouts(Instant::now());
            }

            // 5. Run the focus-controller pipeline.
            wd!().set_stage(LoopStage::AuxIo);
            let focus_transition = {
                let engine_present = self
                    .instance
                    .as_ref()
                    .and_then(|i| i.registry_rust())
                    .map(|r| r.active_keyboard_name().is_some())
                    .unwrap_or(false);
                let frontend = self.frontend.as_mut().unwrap();
                let router = self.router.as_mut().unwrap();
                let timer = self.repeat_timer.as_mut().unwrap();
                let mut transition = None;
                if let Some(ref mut driver) = self.focus_driver {
                    transition = driver.tick(frontend, router, timer, engine_present);
                }
                transition
            };

            // 5b. Translate the focus transition into an indicator trigger.
            //    The focus driver has already applied its effects (grab
            //    build/teardown, anchor reset, panel hide on deactivate);
            //    this only layers the indicator on top.
            if let Some(t) = focus_transition {
                match t {
                    FocusTransition::FirstActivate => {
                        if let Some(wd) = self.watchdog.as_ref() {
                            wd.set_armed(true);
                        }
                        self.trigger_indicator_focus();
                    }
                    FocusTransition::Reactivate => {
                        if let Some(wd) = self.watchdog.as_ref() {
                            wd.set_armed(true);
                        }
                        self.trigger_indicator_reactivate();
                    }
                    FocusTransition::Deactivate => {
                        if let Some(wd) = self.watchdog.as_ref() {
                            wd.set_armed(false);
                        }
                        self.hide_indicator();
                    }
                }
            }

            // 6. Drain engine output and process all pending key events.
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

                router.drain_commit(state);
                router.drain_composition(state);

                let pending_keys = state.take_pending_keys();
                if !pending_keys.is_empty() {
                    tracing::debug!(
                        target: "typio.input.queue",
                        pending_key_count = pending_keys.len(),
                        "drain pending keys"
                    );
                }
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
                        let consumed = match router.try_host_selection(&key, state) {
                            Some(handled) => handled,
                            None => router.dispatch_key(&key, mods),
                        };
                        // Any key that reached the engine (consumed or
                        // forwarded) counts as "user activity" for the
                        // indicator's acknowledged-recency gate. Releases,
                        // modifier-only events, and filtered-out keys do
                        // not (mirrors the C `record_key_activity` caller
                        // in keyboard.c).
                        if let Some(indicator) = self.indicator.as_mut() {
                            indicator.record_key_activity(Instant::now());
                        }
                        if router.take_switch_chord_fired() {
                            // Ctrl+Shift (default) just completed. Cycle
                            // to the next language (reusing the engine last
                            // used for it); with <2 languages the cycle
                            // falls back to engine cycling. Suppresses
                            // forwarding of the modifier press itself.
                            tracing::debug!(target: "typio.indicator", "Ctrl+Shift language-switch chord fired");
                            let instance_ptr = self
                                .instance
                                .as_mut()
                                .map(|i| i.as_mut() as *mut TypioInstance)
                                .unwrap_or(std::ptr::null_mut());
                            cycle_active_language(instance_ptr);
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
                            // long preedit one char per tick).
                            router.drain_commit(state);
                            router.drain_composition(state);
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
                        // Forward release events to the engine so
                        // engines that need them (e.g. Rime schema
                        // switching on a lone Shift release) can
                        // complete gesture detection. Modifier state
                        // is mirrored separately via the Modifiers
                        // grab event, so not forwarding a consumed
                        // release here does not leave a stuck modifier
                        // in the focused app. Host-managed selection
                        // releases are swallowed by `try_host_selection`
                        // so the engine never sees an unpaired release
                        // for a press the host intercepted.
                        let consumed = match router.try_host_selection(&key, state) {
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
                        } else if consumed {
                            router.drain_commit(state);
                            router.drain_composition(state);
                        } else {
                            state.forward_key(key.time, key.keycode, key.state);
                        }
                        router.on_release(&key);
                        let _ = timer.stop();
                    }
                }
            }
            if let Some(label) = voice_status_to_show {
                // Press/release only surface transient feedback here (errors,
                // unavailable); the in-progress banners are driven by the
                // session state machine in `handle_voice_outcome`.
                self.show_voice_transient(label, Instant::now());
            }

            // 7. Flush the candidate panel if the scheduler says so.
            wd!().set_stage(LoopStage::PanelUpdate);
            {
                // Pull the watchdog handle up-front, mirroring
                // `render_indicator_banner`. The heartbeat closure
                // captures only this reference (not all of `self`),
                // which lets it coexist with the mutable `frontend`
                // borrow used to obtain `panel` — a split-borrow
                // requirement that the inline `wd!()` form would
                // trip when nested inside a closure passed to
                // `draw_candidates`.
                let wd_ref = self.watchdog.as_ref();
                let heartbeat = move || {
                    if let Some(wd) = wd_ref {
                        wd.heartbeat();
                    }
                };
                let enter_present = move || {
                    if let Some(wd) = wd_ref {
                        wd.set_stage(LoopStage::Present);
                    }
                };
                heartbeat();

                let frontend = self.frontend.as_mut().unwrap();
                let router = self.router.as_mut().unwrap();
                // Cheap scalars only — avoid cloning the candidates Vec on
                // every tick. The clone is deferred to the rare flush path
                // below; the common Idle or non-flushing tick now pays only
                // three field reads.
                let (schedule_state, selected, composition_seq, candidate_count) = {
                    let state = frontend.state();
                    (
                        state.panel_schedule_state,
                        state.composition.selected_candidate,
                        state.composition.composition_seq,
                        state.composition.candidates.len(),
                    )
                };
                let has_session = router.is_focused();
                let has_context = !router.ctx().is_null();
                // `has_session` and `context_focused` are both
                // `router.is_focused()`; collapse them into one source.
                let context_focused = has_session;
                let should_flush = panel_scheduler::should_flush(
                    schedule_state,
                    has_session,
                    has_context,
                    context_focused,
                );
                if schedule_state != panel_scheduler::PanelScheduleState::Idle {
                    tracing::trace!(
                        target: "typio.panel.scheduler",
                        schedule_state = ?schedule_state,
                        should_flush,
                        composition_seq,
                        candidate_count,
                        selected,
                        has_session,
                        has_context,
                        context_focused,
                        "panel scheduler tick"
                    );
                }
                // Idle-escape valve. A `Dirty` schedule only exits `Dirty`
                // through the flush path below, which is gated on focus +
                // candidates. When neither can ever satisfy the gate this
                // tick — empty candidate list, or the context/session has
                // dropped out from under us (e.g. focus moved to a surface
                // that keeps the input context alive without reporting
                // `is_focused`, the browser case) — the flush block is
                // skipped and the schedule would pin `Dirty` forever,
                // spinning the trace log on every loop iteration. Settling
                // here is safe: there is nothing on screen to lose, and a
                // future `mark_dirty` (candidates arrive, focus returns)
                // re-arms the path.
                if panel_scheduler::should_settle(
                    schedule_state,
                    candidate_count,
                    has_context,
                    has_session,
                ) {
                    frontend.state_mut().panel_schedule_state = panel_scheduler::complete();
                    tracing::debug!(
                        target: "typio.panel.scheduler",
                        composition_seq,
                        candidate_count,
                        has_context,
                        has_session,
                        "panel: settle Dirty → Idle (no flushable work)"
                    );
                }
                if should_flush {
                    // Now that we know we'll actually render, take the
                    // expensive snapshot — the candidate strings, needed
                    // for measurement and text rasterisation.
                    let candidates = frontend.state().composition.candidates.clone();
                    let scale = frontend.state().buffer_scale;
                    let owner = frontend.state().panel_coord().visible_owner();

                    let mut anchor_ready = frontend.state().panel_coord().anchor_ready();

                    // Manage surface ownership before the mutable panel
                    // borrow. Candidates claim the surface when shown
                    // (superseding a visible indicator, so its auto-hide
                    // cannot later kill the candidate list); they release
                    // it when empty unless an overlay is borrowing it.
                    {
                        let state = frontend.state_mut();
                        let owner_changed = {
                            let coord = state.panel_coord_mut();
                            let before = coord.visible_owner();
                            if candidates.is_empty() {
                                if owner != UiOwner::Indicator && owner != UiOwner::Voice {
                                    coord.hide(UiOwner::Candidate);
                                }
                            } else if !anchor_ready {
                                let decision =
                                    coord.decide_positioned_flush(UiOwner::Candidate, "candidate");
                                if decision == crate::panel_coordinator::FlushDecision::Show {
                                    anchor_ready = true;
                                }
                            } else {
                                coord.claim(UiOwner::Candidate);
                            }
                            before != coord.visible_owner()
                        };
                        if owner_changed {
                            state.invalidate_panel_presentation();
                        }
                    }

                    // Frame pacing. The panel renders offscreen and attaches
                    // host-owned SHM buffers on this thread. `wl_surface.frame`
                    // is a refresh hint, not a hard lock: if the compositor
                    // drops callbacks for an input popup, the soft gate wakes
                    // on a timer and submits the latest coalesced candidates
                    // instead of freezing until a long watchdog timeout. Hides
                    // are never throttled — they detach the buffer and cannot
                    // block.
                    //
                    // We still hold candidates if the anchor is not ready, to
                    // avoid committing a popup buffer before the compositor
                    // has sent a text_input_rectangle.
                    let present_decision = if !candidates.is_empty() && anchor_ready {
                        frontend.state_mut().panel_present_decision(Instant::now())
                    } else {
                        PresentDecision::Present
                    };
                    let frame_wait_ms = match present_decision {
                        PresentDecision::Present => None,
                        PresentDecision::WaitUntil(deadline) => {
                            Some(crate::panel_present_gate::deadline_remaining_ms(
                                deadline,
                                Instant::now(),
                            ))
                        }
                    };
                    let throttled =
                        !candidates.is_empty() && (!anchor_ready || frame_wait_ms.is_some());
                    let already_presented = !candidates.is_empty()
                        && anchor_ready
                        && frontend.state().panel_coord().visible_owner() == UiOwner::Candidate
                        && frontend.state().panel_presentation_current(composition_seq);

                    let outcome = if already_presented {
                        tracing::trace!(
                            target: "typio.panel.host",
                            composition_seq,
                            "panel: skip present reason=already_presented"
                        );
                        FlushOutcome::AlreadyPresented
                    } else if throttled {
                        if let Some(wait_ms) = frame_wait_ms {
                            tracing::trace!(
                                target: "typio.panel.host",
                                wait_ms,
                                "panel: skip present reason=frame_soft_gate"
                            );
                        } else {
                            tracing::trace!(
                                target: "typio.panel.host",
                                "panel: skip present reason=anchor_not_ready"
                            );
                        }
                        FlushOutcome::Throttled
                    } else if let Some(panel) = frontend.panel_mut() {
                        panel.set_scale(scale);
                        heartbeat();
                        if candidates.is_empty() {
                            if owner == UiOwner::Indicator || owner == UiOwner::Voice {
                                // Surface is loaned to the indicator/voice
                                // overlay; the candidate path does not own it
                                // right now and must not hide it (would kill
                                // the overlay mid-display).
                                tracing::trace!(
                                    target: "typio.panel.host",
                                    owner = ?owner,
                                    "panel: skip-hide reason=empty_on_loan"
                                );
                                // The surface is non-empty (an overlay owns
                                // it) and the candidate list is empty, so
                                // there is nothing for this path to present
                                // or hide. Settle to Idle: a future candidate
                                // update re-dirties if needed.
                                FlushOutcome::AlreadyPresented
                            } else {
                                tracing::debug!(
                                    target: "typio.panel.host",
                                    owner = ?owner,
                                    "panel: hide reason=candidates_empty"
                                );
                                panel.hide();
                                FlushOutcome::Hidden
                            }
                        } else {
                            panel.ensure_candidate_size(&candidates);
                            heartbeat();
                            // `draw_candidates` returns true only when a
                            // buffer was actually attached to the surface.
                            // A false return means the frame was dropped
                            // before reaching the compositor (dma-buf slot
                            // still held by the compositor, SHM pool
                            // exhausted, or a flux frame begin/submit/export
                            // failure). Treat that the same as throttled:
                            // keep the schedule Dirty so the next tick
                            // re-attempts with the latest coalesced
                            // candidates, rather than marking this
                            // composition_seq done and losing the frame.
                            let attached = panel.draw_candidates(
                                &candidates,
                                selected,
                                composition_seq,
                                &heartbeat,
                                &enter_present,
                            );
                            if attached {
                                FlushOutcome::Presented
                            } else {
                                tracing::debug!(
                                    target: "typio.panel.host",
                                    composition_seq,
                                    "panel: present dropped — will retry next tick"
                                );
                                FlushOutcome::Dropped
                            }
                        }
                    } else {
                        FlushOutcome::NoPanel
                    };
                    match outcome {
                        FlushOutcome::Throttled => {
                            // Leave the schedule dirty so the tick that wakes
                            // on the frame callback, soft-gate deadline, or
                            // anchor fallback re-attempts the present with the
                            // latest coalesced candidates. Do not call
                            // complete(): that would move to Idle and drop the
                            // pending frame.
                        }
                        FlushOutcome::Dropped => {
                            // A present-path drop (draw_candidates attached
                            // nothing even though candidates were non-empty)
                            // must also keep the schedule Dirty: we rendered
                            // but the compositor never got the frame (e.g.
                            // dma-buf slot busy), so marking it presented +
                            // Idle would lose it. Re-arm the next tick
                            // instead, by which point the compositor's
                            // wl_buffer.release has usually arrived.
                            tracing::trace!(
                                target: "typio.panel.host",
                                composition_seq,
                                "panel: keeping schedule dirty after present drop"
                            );
                        }
                        FlushOutcome::NoPanel => {
                            // No surface to draw into; the schedule stays
                            // Dirty so the flush re-attempts once the panel
                            // object is (re)created.
                        }
                        FlushOutcome::Hidden => {
                            frontend.state_mut().clear_panel_frame_callback();
                            frontend.state_mut().invalidate_panel_presentation();
                            frontend.state_mut().panel_schedule_state = panel_scheduler::complete();
                        }
                        FlushOutcome::Presented => {
                            frontend.state_mut().panel_schedule_state = panel_scheduler::complete();
                            frontend.state_mut().mark_panel_presented(composition_seq);
                            frontend.arm_panel_frame_callback();
                        }
                        FlushOutcome::AlreadyPresented => {
                            // Nothing to draw and nothing to retry: the latest
                            // composition_seq is already on screen. Settling
                            // to Idle here is the fix for the busy-loop where
                            // a no-op page flip (same candidates, same seq)
                            // left the schedule pinned Dirty and the
                            // already-presented skip path retried forever.
                            frontend.state_mut().panel_schedule_state = panel_scheduler::complete();
                        }
                    }
                }
            }

            // 7b. Flush any pending positioned status UI (indicator / voice)
            //     when the anchor becomes ready or the caret fallback fires.
            //     Drives the deferred-show path: a `show_on_focus` or
            //     `show_for_state_change` call returned a label, the
            //     coordinator queued it because the anchor wasn't ready,
            //     and now the anchor resolved (or the caret fallback fired).
            {
                let now = Instant::now();
                let flushed = {
                    let frontend = self.frontend.as_mut().unwrap();
                    let state = frontend.state_mut();
                    state.panel_coord_mut().flush_pending_with_timeout(now)
                };
                if let Some((owner, label)) = flushed {
                    tracing::debug!(
                        target: "typio.indicator",
                        ?owner,
                        %label,
                        "deferred flush"
                    );
                    if owner == UiOwner::Indicator {
                        self.render_indicator_banner(&label, now);
                    } else if owner == UiOwner::Voice {
                        let kind = self
                            .voice_pending_banner
                            .take()
                            .unwrap_or(super::indicator::VoiceBanner::Transient);
                        self.render_voice_status_banner(&label, now, kind);
                    } else if owner == UiOwner::Candidate {
                        // The anchor probe timed out, and the caret fallback was armed.
                        // Mark the panel dirty so it redraws with the fallback anchor
                        // on the next iteration.
                        if let Some(frontend) = self.frontend.as_mut() {
                            frontend.state_mut().mark_panel_dirty();
                        }
                    }
                }
            }

            // 8. Repeat timer expiration.
            if fds[2].revents & libc::POLLIN != 0 {
                wd!().set_stage(LoopStage::Repeat);
                let mut buf = [0u8; 8];
                unsafe {
                    libc::read(repeat_fd, buf.as_mut_ptr() as *mut c_void, buf.len());
                }
                let frontend = self.frontend.as_mut().unwrap();
                let state = frontend.state_mut();
                let router = self.router.as_mut().unwrap();
                let timer = self.repeat_timer.as_mut().unwrap();
                let mods = state.mods_depressed;
                match router.dispatch_repeat(state, mods) {
                    RepeatOutcome::Forwarded => {}
                    RepeatOutcome::Consumed => {
                        router.drain_commit(state);
                        router.drain_composition(state);
                    }
                    RepeatOutcome::Stopped => {
                        let _ = timer.stop();
                    }
                }
            }

            // 8b. Indicator auto-hide timer expiration. The timerfd fires
            //     once after `display.indicator_duration_ms`; we hide the
            //     popup and disarm. The indicator's recency tracking is
            //     left intact so a recent indicator still suppresses the
            //     next focus-path reveal.
            if fds[5].revents & libc::POLLIN != 0 {
                let mut buf = [0u8; 8];
                if let Some(tf) = self.indicator_timer.as_ref() {
                    unsafe {
                        libc::read(
                            tf.as_fd().as_raw_fd(),
                            buf.as_mut_ptr() as *mut c_void,
                            buf.len(),
                        );
                    }
                }
                self.hide_indicator();
            }

            // 8c. Voice status auto-hide timer expiration.
            if fds[6].revents & libc::POLLIN != 0 {
                let mut buf = [0u8; 8];
                if let Some(tf) = self.voice_status_timer.as_ref() {
                    unsafe {
                        libc::read(
                            tf.as_fd().as_raw_fd(),
                            buf.as_mut_ptr() as *mut c_void,
                            buf.len(),
                        );
                    }
                }
                self.hide_voice_status();
            }

            // 8d. Voice session events. The session fd signals inference and
            //     async-load completion, so `dispatch` (which reads the fd and
            //     joins the inference thread) only runs when the fd is
            //     readable. But `start`/`stop` queue state transitions
            //     synchronously on push-to-talk press/release, so we drain —
            //     and surface — outcomes every tick to keep the banner in step
            //     with the session state machine. A recognised result is
            //     committed to the focused text field inside the handler.
            if fds[7].revents & libc::POLLIN != 0 {
                if let Some(voice) = self.voice.as_ref() {
                    voice.dispatch();
                }
            }
            let voice_outcomes = self.voice.as_ref().map(|v| v.drain()).unwrap_or_default();
            if !voice_outcomes.is_empty() {
                let now = Instant::now();
                for outcome in voice_outcomes {
                    self.handle_voice_outcome(outcome, now);
                }
            }

            // End-of-tick heartbeat.
            wd!().stage_done();

            // 9. Config watcher events. These are handled after the main
            //    pipeline so a temporary field borrow can be used for the
            //    config reload without colliding with the watchdog macro.
            if fds[3].revents & libc::POLLIN != 0 {
                if let Some(ref mut watcher) = self.config_watcher {
                    match watcher.drain_inotify() {
                        Ok(outcome) => {
                            if outcome.should_rearm_watches {
                                let _ = watcher.rearm_watches();
                            }
                            if outcome.should_schedule_reload {
                                let _ = watcher.schedule_reload();
                            }
                        }
                        Err(e) => {
                            tracing::warn!(target: "typio.config", error = %e, "inotify drain failed")
                        }
                    }
                }
            }
            if fds[4].revents & libc::POLLIN != 0 {
                let should_reload = if let Some(ref mut watcher) = self.config_watcher {
                    match watcher.drain_timer() {
                        Ok(true) => true,
                        Ok(false) => false,
                        Err(e) => {
                            tracing::warn!(target: "typio.config", error = %e, "timer drain failed");
                            false
                        }
                    }
                } else {
                    false
                };
                if should_reload {
                    wd!().set_stage(LoopStage::ConfigReload);
                    self.reload_config();
                }
            }
        }

        tracing::info!(target: "typio.lifecycle", "shutting down");
        0
    }
}
