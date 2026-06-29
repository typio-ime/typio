//! On-screen status indicator driving.
//!
//! All `App` methods that compose, schedule, render, or hide the
//! transient `<badge> · <engine> · <mode>` banner and the voice status
//! banner live here. The loop
//! driver in [`super::mod`] calls into [`App::trigger_indicator_focus`]
//! / [`App::trigger_indicator_reactivate`] / [`App::trigger_indicator_state_change`]
//! / [`App::request_indicator_show`] / [`App::render_indicator_banner`] /
//! [`App::hide_indicator`] / [`App::request_voice_status_show`] /
//! [`App::hide_voice_status`] / [`App::indicator_hide_remaining_ms`].

use std::ffi::CStr;
use std::time::Instant;

use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{Expiration, TimerSetTimeFlags};
use typio_abi::TypioStatusSalience;

use crate::indicator::{EngineModeSnapshot, IndicatorConfig, LabelSources, Salience};
use crate::panel_coordinator::{FlushDecision, UiOwner};
use crate::watchdog::LoopStage;

use typio::TypioInstance;

#[cfg(feature = "wayland")]
use crate::voice::VoiceOutcome;
#[cfg(feature = "wayland")]
use typio::voice::types::VoiceState;

use super::App;

/// Lifetime policy for a voice status banner.
///
/// Voice has two visually identical but behaviourally distinct banners:
/// the in-progress states ("listening…", "transcribing…", "loading…")
/// must persist for as long as the session sits in that state — which is
/// unbounded for push-to-talk — while the terminal feedback (the final
/// transcription, an error, or no-speech) should fade on its own like the
/// keyboard indicator. Conflating the two is what made an active recording
/// vanish after the indicator's auto-hide interval.
#[cfg(feature = "wayland")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VoiceBanner {
    /// Stays until the session state advances or it is explicitly hidden.
    /// No auto-hide timer is armed.
    Sticky,
    /// Auto-hides after the configured indicator duration.
    Transient,
}

/// Which indicator show-path to take. Mirror of
/// [`Indicator`](crate::indicator::Indicator)'s three public methods,
/// lifted into a tag so [`App::trigger_indicator_show`] can dispatch on
/// a single borrow scope without re-borrowing `self` for each arm.
#[cfg(feature = "wayland")]
#[derive(Debug, Clone, Copy)]
pub(super) enum IndicatorPath {
    Focus,
    Reactivate,
    StateChange,
}

/// [`LabelSources`] backed by the live `EngineRegistry`. Borrows its
/// strings so the indicator label composition is zero-allocation on the
/// hot path.
pub(super) struct RegistryLabelSources<'a> {
    pub(super) registry: &'a typio::core::registry::EngineRegistry,
}

impl<'a> LabelSources for RegistryLabelSources<'a> {
    fn active_language_tag(&self) -> Option<&str> {
        self.registry.active_language()
    }
    fn active_engine_name(&self) -> Option<&str> {
        self.registry.active_keyboard_name()
    }
    fn active_engine_display_name(&self) -> Option<&str> {
        self.registry
            .active_keyboard_name()
            .and_then(|name| self.registry.engine_info(name))
            .map(|info| info.display_name.as_str())
    }
}

impl App {
    /// Read `display.indicator_*` from libtypio's config cache. Returns
    /// the default-enabled, default-1500ms config when the instance or
    /// config pointer is unavailable.
    pub(super) fn load_indicator_config(&self) -> IndicatorConfig {
        let raw = match self.instance.as_ref() {
            Some(i) => i.as_ref() as *const TypioInstance as *mut TypioInstance,
            None => return IndicatorConfig::default(),
        };
        let cfg = typio::instance::typio_instance_get_config(raw);
        if cfg.is_null() {
            return IndicatorConfig::default();
        }
        let enabled =
            typio::config::typio_config_get_bool(cfg, c"display.indicator_enabled".as_ptr(), true);
        let duration_ms = typio::config::typio_config_get_int(
            cfg,
            c"display.indicator_duration_ms".as_ptr(),
            1500,
        );
        IndicatorConfig::from_values(enabled, duration_ms.into())
    }

    /// Trigger the indicator's focus-path show (FirstActivate). Reads the
    /// live registry and cached mode from libtypio; applies the salience
    /// gate + acknowledged-recency gate.
    #[cfg(feature = "wayland")]
    pub(super) fn trigger_indicator_focus(&mut self) {
        self.trigger_indicator_show(IndicatorPath::Focus);
    }

    /// Trigger the indicator's reactivate-path show (Reactivate). Gates:
    /// salience only — recency is skipped per ADR-0018.
    #[cfg(feature = "wayland")]
    pub(super) fn trigger_indicator_reactivate(&mut self) {
        self.trigger_indicator_show(IndicatorPath::Reactivate);
    }

    /// Trigger the indicator's deliberate-change show (no gates beyond
    /// `enabled`). Called from the `StateRefresh` drain — covers Ctrl+Shift
    /// chord, tray-driven engine/language switch, and IPC-driven mutations.
    #[cfg(feature = "wayland")]
    pub(super) fn trigger_indicator_state_change(&mut self) {
        self.trigger_indicator_show(IndicatorPath::StateChange);
    }

    /// Shared body of the three trigger paths. Resolves label sources from
    /// the live registry, asks the [`Indicator`](crate::indicator::Indicator)
    /// state machine for a label, and feeds any returned label to
    /// [`Self::request_indicator_show`].
    #[cfg(feature = "wayland")]
    fn trigger_indicator_show(&mut self, path: IndicatorPath) {
        let now = Instant::now();

        // Read the current mode from libtypio's cache. The mode-changed
        // callback stores the fresh mode in `last_mode` before firing, so
        // by the time StateRefresh delivers us here, the data is current.
        // This covers rime's schema/mode switches: the engine reports its
        // new active mode (e.g. display_label="中", salience=Notable), the
        // callback fires, we read it here, and the indicator shows the mode
        // suffix instead of just the bare engine name.
        let mode_display: Option<String> = self.read_mode_display_label();
        let mode_salience = self.read_mode_salience();

        let label = {
            let Some(instance) = self.instance.as_ref() else {
                tracing::debug!(target: "typio.indicator", "no instance, skipping show");
                return;
            };
            let Some(registry) = instance.registry_rust() else {
                tracing::debug!(target: "typio.indicator", "no registry, skipping show");
                return;
            };
            let sources = RegistryLabelSources { registry };
            let Some(indicator) = self.indicator.as_mut() else {
                tracing::debug!(
                    target: "typio.indicator",
                    "no indicator state machine, skipping show"
                );
                return;
            };
            let cfg = self.indicator_config;

            // Build the mode snapshot from the libtypio-cached mode. The
            // snapshot always exists when we have a valid mode pointer,
            // even if `display_label` is None — the salience gate still
            // needs to see it.
            let mode_snapshot = EngineModeSnapshot {
                display_label: mode_display.as_deref(),
                salience: mode_salience,
            };
            let mode_ref = Some(&mode_snapshot);

            let label = match path {
                IndicatorPath::Focus => indicator.show_on_focus(now, mode_ref, &cfg, &sources),
                IndicatorPath::Reactivate => {
                    indicator.show_on_reactivate(now, mode_ref, &cfg, &sources)
                }
                IndicatorPath::StateChange => {
                    indicator.show_for_state_change(now, mode_ref, &cfg, &sources)
                }
            };
            tracing::debug!(
                target: "typio.indicator",
                ?path,
                mode_display = ?mode_display,
                salience = ?mode_salience,
                lang = ?sources.active_language_tag(),
                engine = ?sources.active_engine_name(),
                ?label,
                "show evaluated"
            );
            label
        };
        if let Some(label) = label {
            self.request_indicator_show(label, now);
        }
    }

    /// Read the cached mode's `display_label` from libtypio (e.g. "中",
    /// "A", "Latin"). Returns `None` when no engine has reported a mode
    /// yet, or when the mode has no display label.
    fn read_mode_display_label(&self) -> Option<String> {
        let raw = self.instance.as_ref()?;
        let raw = raw.as_ref() as *const TypioInstance as *mut TypioInstance;
        let mode_ptr = typio::instance::typio_instance_get_last_keyboard_mode(raw);
        if mode_ptr.is_null() {
            return None;
        }
        let mode = unsafe { &*mode_ptr };
        if mode.display_label.is_null() {
            None
        } else {
            Some(
                unsafe { CStr::from_ptr(mode.display_label) }
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    /// Read the cached mode's salience. Returns `Quiet` when no mode is set.
    fn read_mode_salience(&self) -> Salience {
        let raw = match self.instance.as_ref() {
            Some(i) => i.as_ref() as *const TypioInstance as *mut TypioInstance,
            None => return Salience::Quiet,
        };
        let mode_ptr = typio::instance::typio_instance_get_last_keyboard_mode(raw);
        if mode_ptr.is_null() {
            return Salience::Quiet;
        }
        let mode = unsafe { &*mode_ptr };
        match mode.salience {
            TypioStatusSalience::TypioStatusSalienceNotable => Salience::Notable,
            _ => Salience::Quiet,
        }
    }

    /// Feed an indicator show request through the `PanelCoordinator`.
    /// If the anchor is ready the banner renders immediately and the
    /// auto-hide timer is armed; otherwise the coordinator queues the
    /// request and it flushes on a later tick through
    /// `flush_pending_with_timeout`.
    #[cfg(feature = "wayland")]
    pub(super) fn request_indicator_show(&mut self, label: String, now: Instant) {
        let decision = {
            let Some(frontend) = self.frontend.as_mut() else {
                tracing::debug!(target: "typio.indicator", "no frontend, skipping");
                return;
            };
            let coord = frontend.state_mut().panel_coord_mut();
            let anchor_ready = coord.anchor_ready();
            let decision = coord.decide_positioned_flush(UiOwner::Indicator, &label);
            tracing::debug!(
                target: "typio.indicator",
                anchor_ready,
                ?decision,
                "coordinator flush decision"
            );
            decision
        };
        match decision {
            FlushDecision::Show => self.render_indicator_banner(&label, now),
            FlushDecision::Pending => {
                tracing::debug!(
                    target: "typio.indicator",
                    "queued, will flush when anchor resolves"
                );
            }
            FlushDecision::Cancel => {
                tracing::debug!(target: "typio.indicator", "coordinator cancelled the request");
                if let Some(indicator) = self.indicator.as_mut() {
                    indicator.hide();
                }
            }
        }
    }

    /// Show a voice-input status banner. This uses the same positioned popup
    /// surface as the indicator, but claims it under the Voice owner so the
    /// candidate panel will not hide it while no candidates are visible.
    #[cfg(feature = "wayland")]
    pub(super) fn request_voice_status_show(
        &mut self,
        label: String,
        now: Instant,
        kind: VoiceBanner,
    ) {
        let decision = {
            let Some(frontend) = self.frontend.as_mut() else {
                tracing::debug!(target: "typio.voice", "no frontend, skipping status banner");
                return;
            };
            let coord = frontend.state_mut().panel_coord_mut();
            let anchor_ready = coord.anchor_ready();
            let decision = coord.decide_positioned_flush(UiOwner::Voice, &label);
            tracing::debug!(
                target: "typio.voice",
                anchor_ready,
                ?decision,
                ?kind,
                "coordinator flush decision"
            );
            decision
        };
        match decision {
            FlushDecision::Show => self.render_voice_status_banner(&label, now, kind),
            FlushDecision::Pending => {
                // The anchor is not ready yet; the coordinator holds the
                // label and re-emits it via `flush_pending_with_timeout`.
                // Remember the kind so the deferred render keeps the right
                // auto-hide policy.
                self.voice_pending_banner = Some(kind);
                tracing::debug!(
                    target: "typio.voice",
                    "queued status banner, will flush when anchor resolves"
                );
            }
            FlushDecision::Cancel => {
                tracing::debug!(target: "typio.voice", "coordinator cancelled the status banner");
            }
        }
    }

    /// Show a sticky voice banner (no auto-hide): the in-progress states
    /// that must persist until the session advances.
    #[cfg(feature = "wayland")]
    pub(super) fn show_voice_sticky(&mut self, label: impl Into<String>, now: Instant) {
        self.request_voice_status_show(label.into(), now, VoiceBanner::Sticky);
    }

    /// Show a transient voice banner that fades after the configured
    /// duration: terminal feedback (result / error / no-speech).
    #[cfg(feature = "wayland")]
    pub(super) fn show_voice_transient(&mut self, label: impl Into<String>, now: Instant) {
        self.request_voice_status_show(label.into(), now, VoiceBanner::Transient);
    }

    /// Render the indicator banner onto the candidate Panel's surface,
    /// mark the indicator as shown (updates recency tracking), and arm
    /// the auto-hide timer. Called either from
    /// [`Self::request_indicator_show`] when the coordinator accepts
    /// immediately, or from the loop's `flush_pending_with_timeout` path
    /// when a queued show later becomes renderable.
    #[cfg(feature = "wayland")]
    pub(super) fn render_indicator_banner(&mut self, label: &str, now: Instant) {
        self.draw_status_banner_now(label);
        if let Some(indicator) = self.indicator.as_mut() {
            indicator.note_shown(now);
        }
        self.arm_indicator_timer(now);
    }

    /// Render a voice-input status banner onto the shared positioned popup
    /// surface and arm the voice status auto-hide timer. This deliberately
    /// bypasses the keyboard indicator state machine and recency gate.
    #[cfg(feature = "wayland")]
    pub(super) fn render_voice_status_banner(
        &mut self,
        label: &str,
        now: Instant,
        kind: VoiceBanner,
    ) {
        self.draw_status_banner_now(label);
        match kind {
            // Sticky banners must not auto-hide. Disarm any timer left over
            // from a previous transient banner so it cannot clear us.
            VoiceBanner::Sticky => self.disarm_voice_status_timer(),
            VoiceBanner::Transient => self.arm_voice_status_timer(now),
        }
    }

    /// Shared drawing path for status banners. Ownership and timer semantics
    /// stay with the caller; this only sizes, paints, and commits the panel.
    #[cfg(feature = "wayland")]
    fn draw_status_banner_now(&mut self, label: &str) {
        let scale = self
            .frontend
            .as_ref()
            .map(|f| f.state().buffer_scale)
            .unwrap_or(1.0);
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
        if let Some(panel) = self.frontend.as_mut().and_then(|f| f.panel_mut()) {
            panel.set_scale(scale);
            heartbeat();
            panel.ensure_banner_size(label);
            heartbeat();
            panel.draw_status_banner(label, &heartbeat, &enter_present);
        }
        if let Some(frontend) = self.frontend.as_mut() {
            frontend.arm_panel_frame_callback();
        }
    }

    /// Hide the indicator (timer expiry, deactivate, or coordinator
    /// cancel). Clears the indicator's active flag, dismisses any queued
    /// request for the Indicator owner, detaches the popup surface if the
    /// Indicator was the visible owner, and disarms the auto-hide timer.
    /// Leaves the recency edges intact: a recently-shown indicator still
    /// suppresses the next focus-path reveal.
    #[cfg(feature = "wayland")]
    pub(super) fn hide_indicator(&mut self) {
        if let Some(indicator) = self.indicator.as_mut() {
            indicator.hide();
        }
        let status_owned = {
            let Some(frontend) = self.frontend.as_mut() else {
                return;
            };
            let coord = frontend.state_mut().panel_coord_mut();
            let was_visible = coord.visible_owner() == UiOwner::Indicator;
            coord.hide(UiOwner::Indicator);
            was_visible
        };
        if status_owned {
            tracing::debug!(target: "typio.panel.host", "panel: hide reason=indicator_autohide");
            if let Some(frontend) = self.frontend.as_mut() {
                frontend.state_mut().clear_panel_frame_callback();
                if let Some(panel) = frontend.panel_mut() {
                    panel.hide();
                }
            }
        }
        self.disarm_indicator_timer();
    }

    /// Hide the voice status banner without mutating the keyboard/language
    /// indicator state machine or its recency gate.
    #[cfg(feature = "wayland")]
    pub(super) fn hide_voice_status(&mut self) {
        let status_owned = {
            let Some(frontend) = self.frontend.as_mut() else {
                return;
            };
            let coord = frontend.state_mut().panel_coord_mut();
            let was_visible = coord.visible_owner() == UiOwner::Voice;
            coord.hide(UiOwner::Voice);
            was_visible
        };
        if status_owned {
            tracing::debug!(target: "typio.panel.host", "panel: hide reason=voice_status_autohide");
            if let Some(frontend) = self.frontend.as_mut() {
                frontend.state_mut().clear_panel_frame_callback();
                if let Some(panel) = frontend.panel_mut() {
                    panel.hide();
                }
            }
        }
        self.disarm_voice_status_timer();
    }

    /// Drain one voice-session outcome and reflect it on the status banner.
    ///
    /// The banner is driven by the session state machine (see
    /// [`Self::on_voice_state`]) so the in-progress states stay on screen for
    /// their full, unbounded duration; only the terminal `Result`/`Error`
    /// feedback uses the auto-hide timer.
    #[cfg(feature = "wayland")]
    pub(super) fn handle_voice_outcome(&mut self, outcome: VoiceOutcome, now: Instant) {
        match outcome {
            VoiceOutcome::State(state) => self.on_voice_state(state, now),
            VoiceOutcome::Result(text) => {
                if text.is_empty() {
                    // Recognised audio that filtered down to nothing.
                    self.show_voice_transient("Voice: no speech detected", now);
                } else {
                    tracing::debug!(target: "typio.voice", text = %text, "transcription result");
                    if let Some(frontend) = self.frontend.as_mut() {
                        frontend.state_mut().commit_string_and_flush(&text);
                    }
                    self.show_voice_transient(format!("Voice: {text}"), now);
                }
            }
            VoiceOutcome::Error(msg) => {
                tracing::warn!(target: "typio.voice", message = %msg, "transcription error");
                self.show_voice_transient(format!("Voice: {msg}"), now);
            }
        }
    }

    /// Map a voice session state transition onto the status banner.
    ///
    /// The in-progress states render sticky banners (no auto-hide); reaching
    /// `Idle` tears the sticky banner down unless a terminal `Result`/`Error`
    /// banner has already taken over (in which case its own timer owns the
    /// fade), or the session went idle straight out of `Processing` with no
    /// result — i.e. nothing was recognised.
    #[cfg(feature = "wayland")]
    fn on_voice_state(&mut self, state: VoiceState, now: Instant) {
        let prev = self.voice_last_state;
        self.voice_last_state = state;
        match state {
            VoiceState::Loading => self.show_voice_sticky("Voice: loading model…", now),
            VoiceState::Recording => self.show_voice_sticky("Voice: listening…", now),
            VoiceState::Processing => self.show_voice_sticky("Voice: transcribing…", now),
            VoiceState::Idle => {
                if self.voice_status_hide_deadline.is_some() {
                    // A transient result/error banner is already on screen
                    // (it precedes the Idle transition in the same drain);
                    // leave it to fade on its own timer.
                } else if prev == VoiceState::Processing {
                    // Transcription finished without producing any text.
                    self.show_voice_transient("Voice: no speech detected", now);
                } else {
                    self.hide_voice_status();
                }
            }
        }
    }

    /// Arm the auto-hide timer for the indicator's configured duration
    /// (clamped to 100–10000 ms in [`IndicatorConfig`]). Idempotent —
    /// re-arming replaces any prior deadline.
    #[cfg(feature = "wayland")]
    pub(super) fn arm_indicator_timer(&mut self, now: Instant) {
        let duration = self.indicator_config.duration;
        if let Some(tf) = self.indicator_timer.as_ref() {
            let expiration = Expiration::OneShot(TimeSpec::from_duration(duration));
            let _ = tf.set(expiration, TimerSetTimeFlags::empty());
        }
        self.indicator_hide_deadline = Some(now + duration);
    }

    /// Disarm the auto-hide timer. Safe to call on an already-disarmed
    /// timer; arming with a zero `it_value` is the kernel-defined disarm.
    #[cfg(feature = "wayland")]
    pub(super) fn disarm_indicator_timer(&mut self) {
        if let Some(tf) = self.indicator_timer.as_ref() {
            let expiration =
                Expiration::OneShot(TimeSpec::from_duration(std::time::Duration::ZERO));
            let _ = tf.set(expiration, TimerSetTimeFlags::empty());
        }
        self.indicator_hide_deadline = None;
    }

    /// Arm the auto-hide timer for the voice status banner. It uses the same
    /// display duration setting as the keyboard indicator, but a separate
    /// timer/deadline so the two overlays do not clear each other.
    #[cfg(feature = "wayland")]
    pub(super) fn arm_voice_status_timer(&mut self, now: Instant) {
        let duration = self.indicator_config.duration;
        if let Some(tf) = self.voice_status_timer.as_ref() {
            let expiration = Expiration::OneShot(TimeSpec::from_duration(duration));
            let _ = tf.set(expiration, TimerSetTimeFlags::empty());
        }
        self.voice_status_hide_deadline = Some(now + duration);
    }

    /// Disarm the voice status auto-hide timer.
    #[cfg(feature = "wayland")]
    pub(super) fn disarm_voice_status_timer(&mut self) {
        if let Some(tf) = self.voice_status_timer.as_ref() {
            let expiration =
                Expiration::OneShot(TimeSpec::from_duration(std::time::Duration::ZERO));
            let _ = tf.set(expiration, TimerSetTimeFlags::empty());
        }
        self.voice_status_hide_deadline = None;
    }

    /// Remaining milliseconds until the indicator auto-hide deadline, or
    /// `None` when the timer is not armed.
    #[cfg(feature = "wayland")]
    pub(super) fn indicator_hide_remaining_ms(&self, now: Instant) -> Option<i32> {
        self.indicator_hide_deadline
            .and_then(|d| d.checked_duration_since(now))
            .map(|rem| rem.as_millis() as i32)
            .map(|ms| ms.max(0))
    }

    /// Remaining milliseconds until the voice status auto-hide deadline, or
    /// `None` when the timer is not armed.
    #[cfg(feature = "wayland")]
    pub(super) fn voice_status_hide_remaining_ms(&self, now: Instant) -> Option<i32> {
        self.voice_status_hide_deadline
            .and_then(|d| d.checked_duration_since(now))
            .map(|rem| rem.as_millis() as i32)
            .map(|ms| ms.max(0))
    }
}
