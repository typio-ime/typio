//! The Wayland event-loop driver.
//!
//! `App::run_with_wayland` is the main reactor pipeline: flush,
//! prepare-read, dispatch, poll, read, focus-controller, key drain,
//! repeat, panel flush, config-reload.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::ipc_bus::IpcBus;
use crate::panel_coordinator::UiOwner;
use crate::session_glue::FocusTransition;

use super::App;
use super::panel_driver::flush_candidate_panel;
use super::reactor::{PollSource, PollSourceFds, PollSources, PollTimeout};

impl App {
    /// The Wayland main loop. Returns the daemon exit code.
    pub(super) fn run_with_wayland(&mut self, ipc_bus: Option<&Rc<RefCell<IpcBus>>>) -> i32 {
        let wl_fd = self.frontend.as_mut().unwrap().fd();
        let uds_fd = ipc_bus.map(|ipc| ipc.borrow().epoll_fd()).unwrap_or(-1);
        let repeat_fd = self.repeat_timer.as_mut().unwrap().fd();

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
            .map(|timer| timer.fd())
            .unwrap_or(-1);
        let voice_status_fd = self
            .voice_status_timer
            .as_ref()
            .map(|timer| timer.fd())
            .unwrap_or(-1);
        // Voice session eventfd: becomes readable when inference completes
        // (or an async model load resolves). Stable for the session
        // lifetime, so it can join the static poll set.
        let voice_session_fd = self.voice.as_ref().map(|v| v.session_fd()).unwrap_or(-1);

        let mut poll_sources = PollSources::new(PollSourceFds {
            wayland: wl_fd,
            uds: uds_fd,
            key_repeat: repeat_fd,
            config_watch: inotify_fd,
            config_timer: cfg_timer_fd,
            indicator_timer: indicator_fd,
            voice_timer: voice_status_fd,
            voice_session: voice_session_fd,
            reactor_wake: self.event_waker.fd(),
        });

        while !self.drain_events() {
            // 1. Start-of-step fact bookkeeping.
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
            {
                let frontend = self.frontend.as_ref().unwrap();
                if let Err(e) = frontend.flush() {
                    tracing::error!(target: "typio.wayland.io", error = %e, "flush failed");
                    return 1;
                }
            }
            let (read_guard, did_dispatch) = {
                let frontend = self.frontend.as_mut().unwrap();
                match frontend.prepare_read_loop() {
                    Ok(res) => (Some(res.0), res.1),
                    Err(e) => {
                        tracing::error!(target: "typio.wayland.io", error = %e, "prepare_read failed");
                        return 1;
                    }
                }
            };
            // 3. Poll. Let panel/anchor/status deadlines shorten the timeout.
            let timeout = {
                let frontend = self.frontend.as_ref().unwrap();
                let state = frontend.state();
                let mut timeout = PollTimeout::default();
                let now = Instant::now();
                if let Some(remaining) = state.panel_coord.anchor_deadline_remaining_ms(now) {
                    timeout.reduce(remaining as i32);
                }
                if let Some(remaining) = state.wayland_pending.min_deadline_ms(now) {
                    timeout.reduce(remaining);
                }
                if let Some(remaining) = self
                    .router
                    .as_ref()
                    .and_then(|router| router.preedit_deadline_remaining_ms(now))
                {
                    timeout.reduce(remaining);
                }
                if did_dispatch {
                    timeout.reduce(0);
                }
                timeout
            };
            let ready = match poll_sources.wait(timeout) {
                Ok(ready) => ready,
                Err(e) => {
                    if e.raw_os_error() == Some(libc::EINTR) {
                        continue;
                    }
                    tracing::error!(target: "typio.lifecycle", error = %e, "poll failed");
                    return 1;
                }
            };

            // A Unix signal or cross-thread DaemonEvent must take priority
            // over ordinary Wayland work. Continuing drops `read_guard`
            // (cancelling the prepared Wayland read), then the loop-head
            // drain consumes the eventfd, flags, and typed event queue.
            if ready.readable(PollSource::ReactorWake) {
                continue;
            }

            // 4. Read and dispatch new Wayland events, or cancel the prepared read.
            if ready.readable(PollSource::Wayland) {
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
            } else if ready.disconnected(PollSource::Wayland) {
                tracing::error!(target: "typio.wayland.io", "display disconnected");
                return 1;
            }
            // If POLLIN was not set, `read_guard` is dropped here and cancels the read.

            // 4a. The UDS epoll fd is itself a reactor source. Dispatch it
            // after input events, but in the same step that observed
            // readiness instead of deferring client work to the next step.
            if ready.readable(PollSource::Uds) {
                if let Some(ipc_bus) = ipc_bus {
                    ipc_bus.borrow_mut().dispatch();
                }
            }

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
                        self.trigger_indicator_focus();
                    }
                    FocusTransition::Reactivate => {
                        self.trigger_indicator_reactivate();
                    }
                    FocusTransition::Deactivate => {
                        self.hide_indicator();
                    }
                }
            }

            // 6. Apply the ordered keyboard/text pipeline.
            self.drive_pending_keys(Instant::now());
            // 7. Apply a ready key-repeat expiration before Panel presentation.
            self.drive_repeat(ready.readable(PollSource::KeyRepeat), Instant::now());

            // 8. Converge the candidate Panel after input and repeat updates.
            flush_candidate_panel(
                self.frontend.as_mut().unwrap(),
                self.router.as_ref().unwrap(),
            );

            // 8b. Flush any pending positioned status UI (indicator / voice)
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
                        self.render_voice_status_banner(&label, kind);
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

            // 9. Indicator auto-hide timer expiration. The timerfd fires
            //     once after `display.indicator_duration_ms`; we hide the
            //     popup and disarm. The indicator's recency tracking is
            //     left intact so a recent indicator still suppresses the
            //     next focus-path reveal.
            if ready.readable(PollSource::IndicatorTimer) {
                if let Some(timer) = self.indicator_timer.as_mut() {
                    match timer.consume_expiration() {
                        Ok(_) => self.hide_indicator(),
                        Err(error) => tracing::warn!(
                            target: "typio.indicator",
                            %error,
                            "failed to consume indicator timer expiration"
                        ),
                    }
                }
            }

            // 10. Voice status auto-hide timer expiration.
            if ready.readable(PollSource::VoiceTimer) {
                if let Some(timer) = self.voice_status_timer.as_mut() {
                    match timer.consume_expiration() {
                        Ok(_) => self.hide_voice_status(),
                        Err(error) => tracing::warn!(
                            target: "typio.voice",
                            %error,
                            "failed to consume voice timer expiration"
                        ),
                    }
                }
            }

            // 11. Voice session events. The session fd signals inference and
            //     async-load completion, so `dispatch` (which reads the fd and
            //     joins the inference thread) only runs when the fd is
            //     readable. But `start`/`stop` queue state transitions
            //     synchronously on push-to-talk press/release, so we drain —
            //     and surface — outcomes every reactor step to keep the banner
            //     synchronized
            //     with the session state machine. A recognised result is
            //     committed to the focused text field inside the handler.
            if ready.readable(PollSource::VoiceSession) {
                if let Some(voice) = self.voice.as_ref() {
                    voice.dispatch();
                }
            }
            let voice_outcomes = self.voice.as_ref().map(|v| v.drain()).unwrap_or_default();
            if !voice_outcomes.is_empty() {
                for outcome in voice_outcomes {
                    self.handle_voice_outcome(outcome);
                }
            }

            // 12. Config watcher events. These are handled after the main
            //    pipeline so a temporary field borrow can be used for the
            //    config reload.
            if ready.readable(PollSource::ConfigWatch) {
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
            if ready.readable(PollSource::ConfigTimer) {
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
                    self.reload_config();
                }
            }
        }

        tracing::info!(target: "typio.lifecycle", "shutting down");
        0
    }
}
