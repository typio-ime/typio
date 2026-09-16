//! Wayland input-method frontend — the daemon's entry point to the
//! compositor's keyboard event stream.
//!
//! [`InputMethodState`] binds the globals the daemon needs (`wl_seat`,
//! `zwp_input_method_manager_v2`, `zwp_virtual_keyboard_manager_v1`,
//! `wl_shm`, `wp_viewporter`, `wp_fractional_scale_manager_v1`) and implements
//! `Dispatch` for every protocol object it binds. It owns:
//!
//! - the input-method lifecycle proxy + serial-commit tracking;
//! - the keyboard grab + virtual keyboard bridge;
//! - xkbcommon state for resolving keymap/keysym;
//! - the candidate-popup `wl_surface`, its `wl_shm` release registry, and
//!   panel runtime state (composition projection, render schedule,
//!   presentation de-duplication, popup ownership/anchor arbitration via
//!   [`PanelCoordinator`]).
//!
//! This module is the Wayland protocol surface only. Post-dispatch routing
//! (focus controller, engine session, candidate panel) lives in the app
//! layer and drives this state through its `pub` accessors.
//!
//! The serial-commit protocol increments the serial on every `done`; a
//! commit before the first `done` is silently dropped.

use std::collections::BTreeSet;
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::time::Instant;

use wayland_backend::client::ReadEventsGuard;
use wayland_client::globals::{Global, GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::{wl_keyboard, wl_registry, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use crate::InputFacts;
use crate::Modifiers;
use crate::panel::FluxPanel;
use crate::panel_coordinator::PanelCoordinator;
use crate::panel_present_gate::PresentationRecord;
use crate::panel_scheduler::PanelScheduleState;
use crate::protocols::fractional_scale_v1::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use crate::protocols::fractional_scale_v1::wp_fractional_scale_v1::{self, WpFractionalScaleV1};
use crate::protocols::input_method_v2::zwp_input_method_keyboard_grab_v2::{
    self, ZwpInputMethodKeyboardGrabV2,
};
use crate::protocols::input_method_v2::zwp_input_method_manager_v2::ZwpInputMethodManagerV2;
use crate::protocols::input_method_v2::zwp_input_method_v2::{self, ZwpInputMethodV2};
use crate::protocols::input_method_v2::zwp_input_popup_surface_v2::{self, ZwpInputPopupSurfaceV2};
use crate::protocols::viewporter::wp_viewport::WpViewport;
use crate::protocols::viewporter::wp_viewporter::WpViewporter;
use crate::protocols::virtual_keyboard_v1::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use crate::protocols::virtual_keyboard_v1::zwp_virtual_keyboard_v1::{self, ZwpVirtualKeyboardV1};

/// A decoded key event with xkbcommon-resolved keysym.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedKeyEvent {
    /// Raw keycode from the compositor (evdev scancode).
    pub keycode: u32,
    /// XKB keycode (raw + 8). xkbcommon uses this internally.
    pub xkb_keycode: u32,
    /// Resolved keysym (e.g. XKB_KEY_a = 0x0061).
    pub keysym: u32,
    /// UTF-8 text produced by this key, if any (from xkb_state_key_get_utf8).
    pub unicode: String,
    /// Press (1) or release (0).
    pub state: u32,
    /// Timestamp in milliseconds (from the compositor).
    pub time: u32,
}

/// A modifier sample in both compositor and host layouts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardModifiers {
    pub depressed: u32,
    pub latched: u32,
    pub locked: u32,
    pub group: u32,
    pub effective: Modifiers,
}

/// Keyboard transport events, retained in compositor arrival order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyboardInput {
    Key {
        key: DecodedKeyEvent,
        epoch: u64,
        modifiers: Modifiers,
    },
    Modifiers(KeyboardModifiers),
    Boundary,
}

/// Pending/current text-state carried across the input-method `done` boundary.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// The compositor says this text field is active.
    pub active: bool,
    /// Surrounding text from the focused application.
    pub surrounding_text: Option<String>,
    /// Cursor position in characters.
    pub cursor: u32,
    /// Anchor position in characters.
    pub anchor: u32,
    /// Content hint from the focused application.
    pub content_hint: u32,
    /// Content purpose from the focused application.
    pub content_purpose: u32,
    /// `text_change_cause` from the focused application.
    pub text_change_cause: u32,
}

/// Engine composition projection: what the panel should render and
/// what commit text (if any) is pending. Updated atomically by
/// `KeyboardRouter::drain_composition` (engine → host) and by
/// `KeyboardRouter::try_host_selection` (host-local highlight moves
/// under ADR-0012). The host's preedit text is *not* part of this
/// struct: it is sent through the text-input transaction path and tracked by
/// `KeyboardRouter::preedit_tracking`.
#[derive(Debug, Default)]
pub struct CompositionState {
    /// Current candidate list for the panel to render.
    pub candidates: Vec<String>,
    /// Index of the highlighted candidate.
    pub selected_candidate: usize,
    /// Whether the engine reports a previous candidate page.
    pub has_prev_candidates: bool,
    /// Whether the engine reports a next candidate page.
    pub has_next_candidates: bool,
    /// Engine-declared host-managed-selection flags (ADR-0012). When
    /// non-empty, the host intercepts the corresponding
    /// navigation/selection keys via [`crate::candidate_guard`] instead
    /// of forwarding them to `process_key`. Empty (opt-out) by default.
    pub host_managed_selection: crate::HostSelectionFlags,
    /// Monotonic sequence bumped on every composition change — including
    /// host-local highlight moves — so observers can dedupe.
    pub composition_seq: u64,
    /// Pending commit text for the next flush. Set by the engine commit
    /// callback; taken by the event loop after each dispatch round.
    pub pending_commit: Option<String>,
}

impl CompositionState {
    /// Set the current candidate list + selected index. Bumps the composition
    /// sequence only when the visual candidate state changes, and returns the
    /// current value for logging.
    pub fn set_candidates(&mut self, candidates: Vec<String>, selected: usize) -> u64 {
        if self.candidates == candidates && self.selected_candidate == selected {
            return self.composition_seq;
        }
        self.composition_seq = self.composition_seq.wrapping_add(1);
        self.candidates = candidates;
        self.selected_candidate = selected;
        self.composition_seq
    }

    /// Update candidate-page availability metadata used by host-managed
    /// boundary navigation.
    pub fn set_candidate_page_state(&mut self, has_prev: bool, has_next: bool) {
        self.has_prev_candidates = has_prev;
        self.has_next_candidates = has_next;
    }

    /// Reset all composition state (focus lost / composition discarded).
    pub fn clear(&mut self) {
        self.candidates.clear();
        self.selected_candidate = 0;
        self.has_prev_candidates = false;
        self.has_next_candidates = false;
        self.host_managed_selection = crate::HostSelectionFlags::empty();
        self.pending_commit = None;
        // Note: composition_seq is monotonic across resets so observers
        // don't see a seq regression; do not bump or zero it here.
    }

    /// Take the pending commit text, if any.
    pub fn take_pending_commit(&mut self) -> Option<String> {
        self.pending_commit.take()
    }

    /// Stage a commit text from the engine.
    pub fn set_pending_commit(&mut self, text: String) {
        self.pending_commit = Some(text);
    }
}

/// Wayland state — all bound protocol objects + tracking fields.
///
/// This struct implements `Dispatch` for every protocol object the
/// frontend binds. The `EventQueue` operates on it directly.
///
/// Several fields are **lifetime anchors**, not inputs to logic: a bound
/// Wayland proxy must stay alive and owned for the interface to remain valid
/// (dropping it destroys the proxy). They are never read, which is why the
/// struct carries a targeted `dead_code` allow rather than each anchor
/// pretending to be used.
#[allow(dead_code, reason = "bound Wayland proxies kept alive for ownership")]
struct WaylandObjects {
    seat: wl_seat::WlSeat,
    input_method: ZwpInputMethodV2,
    keyboard_grab: Option<ZwpInputMethodKeyboardGrabV2>,
    virtual_keyboard: ZwpVirtualKeyboardV1,
    compositor: WlCompositor,
    popup_surface_obj: wl_surface::WlSurface,
    popup_surface: ZwpInputPopupSurfaceV2,
    viewporter: Option<WpViewporter>,
    panel_viewport: Option<WpViewport>,
    fractional_scale_manager: Option<WpFractionalScaleManagerV1>,
    panel_fractional_scale: Option<WpFractionalScaleV1>,
    shm: Option<wl_shm::WlShm>,
}

pub struct InputMethodState {
    wayland: Option<WaylandObjects>,
    /// Last candidate composition snapshot successfully submitted to Flux.
    ///
    /// Lets the render path consume only the newest composition and skip
    /// duplicate presents after unrelated wakeups, while still allowing
    /// scale/ownership/hide changes to invalidate the cached submission.
    panel_presentation: PresentationRecord,
    /// Text input rectangle from the compositor (cursor position).
    pub text_input_rect: Option<(i32, i32, i32, i32)>,
    /// Engine composition projection (candidates, selection, commit).
    pub composition: CompositionState,
    serial: u32,
    active: bool,
    initialized: bool,
    /// xkbcommon context — created once, reused across keymap changes.
    xkb_context: xkbcommon::xkb::Context,
    /// Current keymap state — set when the compositor sends a keymap fd.
    /// None until the first keymap event.
    xkb_state: Option<xkbcommon::xkb::State>,
    /// Current keymap keymap — kept alive alongside the state.
    xkb_keymap: Option<xkbcommon::xkb::Keymap>,
    /// Physical modifier state from the latest modifiers event.
    pub mods_depressed: u32,
    pub mods_latched: u32,
    pub mods_locked: u32,
    /// One ordered stream for active and inactive keyboard routing.
    pending_input: Vec<KeyboardInput>,
    /// Identifies the current focus/keymap/grab epoch. Queued old presses
    /// cannot acquire ownership in a new field.
    keyboard_epoch: u64,
    last_key_time: u32,
    /// The sole record of presses delivered to the virtual keyboard.
    forwarded_keys: BTreeSet<u32>,
    /// Physical presses observed in this epoch, including consumed keys.
    held_keys: BTreeSet<u32>,
    #[cfg(test)]
    keyboard_output: Vec<KeyboardInput>,
    /// Raw input facts recorded during this reactor step for the focus controller.
    pub facts: InputFacts,
    /// Set when the compositor declares the input method unavailable.
    stopped: bool,
    /// Pending text state accumulated since the previous `done`.
    pending: SessionState,
    /// Text state committed by the latest `done`.
    current: SessionState,
    /// True once a keymap event has been received for the current grab epoch.
    pub keymap_received_this_epoch: bool,
    /// Panel redraw scheduling state.
    pub panel_schedule_state: PanelScheduleState,
    /// Panel coordinator: anchor probing, caret fallback, and popup ownership.
    pub panel_coord: PanelCoordinator,
    pub buffer_scale: f32,
    /// Latest compositor-reported repeat info `(rate, delay_ms)` from the
    /// grab's `repeat_info` event. `None` until the compositor sends it;
    /// the host falls back to X-server defaults ([`crate::repeat_timer`]
    /// constants) until then. A rate of `0` is the protocol signal for
    /// "do not repeat".
    pub compositor_repeat_info: Option<(i32, i32)>,
    /// Tracks in-flight Wayland requests awaiting a compositor response
    /// (`grab_keyboard`→`keymap` and probe→`text_input_rectangle`). Diagnoses
    /// "compositor did not send X" stalls; see [`crate::wayland_pending`].
    pub wayland_pending: crate::wayland_pending::PendingRequestTracker,
}

impl InputMethodState {
    /// Create a headless, transport-free state instance for testing and mock runs.
    pub fn new_headless() -> Self {
        Self {
            wayland: None,
            panel_presentation: PresentationRecord::default(),
            text_input_rect: None,
            composition: CompositionState::default(),
            serial: 0,
            active: false,
            initialized: false,
            xkb_context: xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS),
            xkb_state: None,
            xkb_keymap: None,
            mods_depressed: 0,
            mods_latched: 0,
            mods_locked: 0,
            pending_input: Vec::new(),
            keyboard_epoch: 1,
            last_key_time: 0,
            forwarded_keys: BTreeSet::new(),
            held_keys: BTreeSet::new(),
            #[cfg(test)]
            keyboard_output: Vec::new(),
            facts: InputFacts::default(),
            stopped: false,
            pending: SessionState::default(),
            current: SessionState::default(),
            keymap_received_this_epoch: false,
            panel_schedule_state: PanelScheduleState::default(),
            panel_coord: PanelCoordinator::new(),
            buffer_scale: 1.0,
            compositor_repeat_info: None,
            wayland_pending: crate::wayland_pending::PendingRequestTracker::default(),
        }
    }

    /// Current protocol serial.
    pub fn serial(&self) -> u32 {
        self.serial
    }

    /// True iff the compositor has activated us.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Current modifier state in the host-wide [`Modifiers`] layout.
    ///
    /// The compositor's `mods_depressed` wire value is a *keymap-dependent*
    /// xkb mod-index mask whose bit positions do not match the host
    /// [`Modifiers`] constants: on a conventional keymap xkb puts NumLock at
    /// `1 << 4` and Super (Mod4) at `1 << 6`, while the host constants expect
    /// Super at `1 << 4` and NumLock at `1 << 5`. Feeding the raw wire mask
    /// into host policy therefore mis-reads Super (never suppresses
    /// auto-repeat, invisible to chord detection) and hallucinates NumLock.
    /// Querying the xkb state by modifier *name* maps the bits correctly for
    /// any keymap. Physically-held modifiers come from the depressed
    /// component; lock states come from the locked component so engines keep
    /// seeing Caps/Num lock.
    ///
    /// Before the first keymap arrives there is no xkb state to query; fall
    /// back to the conventional xkb wire layout, which every mainstream
    /// keymap uses.
    pub fn effective_modifiers(&self) -> Modifiers {
        // Conventional xkb wire bit positions (mod indices in a standard
        // keymap): Shift=0, Lock=1, Control=2, Mod1(Alt)=3, Mod2(NumLock)=4,
        // Mod4(Super)=6.
        const XKB_SHIFT: u32 = 1 << 0;
        const XKB_CAPS: u32 = 1 << 1;
        const XKB_CTRL: u32 = 1 << 2;
        const XKB_ALT: u32 = 1 << 3;
        const XKB_NUM: u32 = 1 << 4;
        const XKB_SUPER: u32 = 1 << 6;

        let Some(ref xs) = self.xkb_state else {
            let mut bits = 0;
            for (wire, host) in [
                (XKB_SHIFT, Modifiers::SHIFT),
                (XKB_CTRL, Modifiers::CTRL),
                (XKB_ALT, Modifiers::ALT),
                (XKB_SUPER, Modifiers::SUPER),
            ] {
                if self.mods_depressed & wire != 0 {
                    bits |= host.0;
                }
            }
            for (wire, host) in [
                (XKB_CAPS, Modifiers::CAPSLOCK),
                (XKB_NUM, Modifiers::NUMLOCK),
            ] {
                if self.mods_locked & wire != 0 {
                    bits |= host.0;
                }
            }
            return Modifiers(bits);
        };

        use xkbcommon::xkb;
        let mut bits = 0;
        for (name, host) in [
            (xkb::MOD_NAME_SHIFT, Modifiers::SHIFT),
            (xkb::MOD_NAME_CTRL, Modifiers::CTRL),
            (xkb::MOD_NAME_ALT, Modifiers::ALT),
            (xkb::MOD_NAME_LOGO, Modifiers::SUPER),
        ] {
            if xs.mod_name_is_active(name, xkb::STATE_MODS_DEPRESSED) {
                bits |= host.0;
            }
        }
        for (name, host) in [
            (xkb::MOD_NAME_CAPS, Modifiers::CAPSLOCK),
            (xkb::MOD_NAME_NUM, Modifiers::NUMLOCK),
        ] {
            if xs.mod_name_is_active(name, xkb::STATE_MODS_LOCKED) {
                bits |= host.0;
            }
        }
        Modifiers(bits)
    }

    /// Mutable access to the raw input facts for this reactor step.
    pub fn facts_mut(&mut self) -> &mut InputFacts {
        &mut self.facts
    }

    /// Take the recorded facts so the focus controller can consume them.
    pub fn take_facts(&mut self) -> InputFacts {
        let mut facts = std::mem::take(&mut self.facts);
        facts.im_is_active = self.active;
        facts
    }

    /// True if the compositor declared the input method unavailable.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Current text-state snapshot committed by the latest `done`.
    pub fn current_session(&self) -> &SessionState {
        &self.current
    }

    /// Mark the candidate panel dirty so the event loop flushes it.
    pub fn mark_panel_dirty(&mut self) {
        self.panel_schedule_state.mark_dirty();
    }

    /// Whether `composition_seq` is already visible for the current panel
    /// presentation generation.
    pub fn panel_presentation_current(&self, composition_seq: u64) -> bool {
        self.panel_presentation.is_current(composition_seq)
    }

    /// Record that `composition_seq` was successfully submitted.
    pub fn mark_panel_presented(&mut self, composition_seq: u64) {
        self.panel_presentation.mark_presented(composition_seq);
    }

    /// Invalidate the last-presented marker after any non-composition change
    /// that still requires repainting the current candidates.
    pub fn invalidate_panel_presentation(&mut self) {
        self.panel_presentation.invalidate();
    }

    fn update_panel_scale(&mut self, new_scale: f32) {
        if !new_scale.is_finite()
            || new_scale <= 0.0
            || (self.buffer_scale - new_scale).abs() < f32::EPSILON
        {
            return;
        }
        self.buffer_scale = new_scale;
        self.invalidate_panel_presentation();
        if !self.composition.candidates.is_empty() {
            self.mark_panel_dirty();
        }
    }

    /// The bound `wl_shm` global, if the compositor advertises it.
    pub fn shm(&self) -> Option<&wl_shm::WlShm> {
        self.wayland.as_ref().and_then(|w| w.shm.as_ref())
    }

    /// The attached `wp_viewport`, if viewporter is supported.
    pub fn panel_viewport(&self) -> Option<&WpViewport> {
        self.wayland
            .as_ref()
            .and_then(|w| w.panel_viewport.as_ref())
    }

    /// Reset the positioned-popup anchor generation. Call on focus-in and
    /// active-to-active handoffs so the next caret rect belongs to a new
    /// generation. Resetting the anchor also invalidates the submitted Panel
    /// snapshot: the compositor may have unmapped the popup during focus churn
    /// even though the candidate content and sequence number did not change.
    pub fn reset_panel_anchor(&mut self) {
        self.panel_coord.reset_anchor();
        refresh_panel_after_anchor_reset(
            &mut self.panel_presentation,
            &mut self.panel_schedule_state,
            !self.composition.candidates.is_empty(),
        );
    }

    /// Clear the cached caret-rect flag.
    pub fn clear_caret_rect(&mut self) {
        self.panel_coord.clear_caret_rect();
    }

    /// Send an anchor probe (empty preedit + commit) to force the compositor
    /// to emit a fresh `text_input_rectangle` for this popup.
    pub fn probe_anchor(&mut self) {
        if self.panel_coord.should_probe_anchor() {
            self.text_transaction_and_flush(None, Some(("", 0)));
            self.panel_coord.record_probe_sent();
            self.wayland_pending.note_probe_sent(Instant::now());
        }
    }

    /// Mutable access to the panel coordinator.
    pub fn panel_coord_mut(&mut self) -> &mut PanelCoordinator {
        &mut self.panel_coord
    }

    /// Immutable access to the panel coordinator.
    pub fn panel_coord(&self) -> &PanelCoordinator {
        &self.panel_coord
    }

    pub fn clear_panel_state(&mut self) {
        self.composition.clear();
        self.panel_schedule_state.complete();
        self.invalidate_panel_presentation();
    }

    /// Whether a keyboard grab object currently exists.
    pub fn keyboard_grab_present(&self) -> bool {
        self.wayland
            .as_ref()
            .and_then(|w| w.keyboard_grab.as_ref())
            .is_some()
    }

    /// Create a new keyboard grab from the input-method object.
    pub fn create_keyboard_grab(&mut self, qh: &QueueHandle<Self>) {
        if self.keyboard_grab_present() {
            return;
        }
        tracing::debug!(target: "typio.wayland.grab", "create");
        if let Some(ref mut wayland) = self.wayland {
            wayland.keyboard_grab = Some(wayland.input_method.grab_keyboard(qh, ()));
        }
        self.keymap_received_this_epoch = false;
        self.wayland_pending.note_grab_sent(Instant::now());
    }

    /// Destroy the current keyboard grab object.
    pub fn destroy_keyboard_grab(&mut self) {
        self.release_forwarded_keys();
        self.pending_input.clear();
        self.keyboard_boundary();
        if let Some(wayland) = self.wayland.as_mut() {
            if let Some(grab) = wayland.keyboard_grab.take() {
                grab.release();
            }
        }
        self.keymap_received_this_epoch = false;
        self.wayland_pending.note_keymap_received(Instant::now());
    }

    /// Commit non-text input-method protocol state to the compositor.
    ///
    /// Text updates must use [`Self::text_transaction_and_flush`] so commit text
    /// and preedit are ordered and coalesced in one transaction.  This helper is
    /// reserved for lifecycle/focus state where no text payload is staged.
    /// Silently dropped before the first `done` — matching the C serial
    /// chokepoint.
    pub fn commit_protocol_state(&mut self) {
        if !self.initialized {
            return;
        }
        if let Some(ref wayland) = self.wayland {
            wayland.input_method.commit(self.serial);
        }
    }

    /// Emit a paired virtual-keyboard event. Orphan and duplicate events are
    /// suppressed for every caller, including inactive pass-through.
    pub fn forward_key(&mut self, time: u32, key: u32, state: u32) {
        let emit = if state == 1 {
            self.forwarded_keys.insert(key)
        } else {
            self.forwarded_keys.remove(&key)
        };
        if emit {
            self.emit_key(time, key, state);
        }
    }

    fn emit_key(&mut self, time: u32, key: u32, state: u32) {
        #[cfg(test)]
        self.keyboard_output.push(KeyboardInput::Key {
            key: DecodedKeyEvent {
                keycode: key,
                xkb_keycode: key + 8,
                keysym: 0,
                unicode: String::new(),
                state,
                time,
            },
            epoch: self.keyboard_epoch,
            modifiers: Modifiers::NONE,
        });
        if let Some(wayland) = self.wayland.as_ref() {
            wayland.virtual_keyboard.key(time, key, state);
        }
    }

    pub fn key_is_forwarded(&self, key: u32) -> bool {
        self.forwarded_keys.contains(&key)
    }

    /// Release all application-owned presses at a focus/keymap/grab boundary.
    pub fn release_forwarded_keys(&mut self) {
        for key in std::mem::take(&mut self.forwarded_keys) {
            self.emit_key(self.last_key_time, key, 0);
        }
    }

    /// Forward an ordered modifier sample, never the final state of a batch.
    pub fn forward_modifiers(&mut self, sample: KeyboardModifiers) {
        #[cfg(test)]
        self.keyboard_output.push(KeyboardInput::Modifiers(sample));
        if let Some(wayland) = self.wayland.as_ref() {
            wayland.virtual_keyboard.modifiers(
                sample.depressed,
                sample.latched,
                sample.locked,
                sample.group,
            );
        }
    }

    /// Raw pointer to the popup wl_surface. Use this to create a FluxPanel on
    /// the SAME surface so panel rendering and popup positioning share one
    /// wl_surface.
    pub fn popup_surface_raw_ptr(&self) -> *mut std::ffi::c_void {
        self.wayland
            .as_ref()
            .map(|w| w.popup_surface_obj.id().as_ptr() as *mut std::ffi::c_void)
            .unwrap_or(core::ptr::null_mut())
    }

    /// Set the current candidate list + selected index for the panel.
    /// Forwards to [`CompositionState::set_candidates`].
    pub fn set_candidates(&mut self, candidates: Vec<String>, selected: usize) -> u64 {
        self.composition.set_candidates(candidates, selected)
    }

    pub fn take_pending_input(&mut self) -> Vec<KeyboardInput> {
        std::mem::take(&mut self.pending_input)
    }

    /// Apply transport effects in FIFO order. Only current, active keys are
    /// returned for host/engine routing. Boundaries and modifier samples also
    /// reach the host so it can cancel gestures and repeats in the same order.
    pub fn prepare_input(&mut self, input: KeyboardInput) -> Option<KeyboardInput> {
        match &input {
            KeyboardInput::Boundary => self.release_forwarded_keys(),
            KeyboardInput::Modifiers(sample) => {
                if self.keymap_received_this_epoch {
                    self.forward_modifiers(*sample);
                }
            }
            KeyboardInput::Key { key, epoch, .. } => {
                if *epoch != self.keyboard_epoch {
                    tracing::debug!(target: "typio.input.keyboard", epoch,
                        current_epoch = self.keyboard_epoch, keycode = key.keycode,
                        state = key.state, "fence obsolete keyboard event");
                    if key.state == 0 {
                        self.forward_key(key.time, key.keycode, 0);
                    }
                    return None;
                }
                if !self.keymap_received_this_epoch {
                    return None;
                }
                if !self.active {
                    self.forward_key(key.time, key.keycode, key.state);
                    return None;
                }
            }
        }
        Some(input)
    }

    pub fn keyboard_epoch(&self) -> u64 {
        self.keyboard_epoch
    }

    pub fn key_repeats(&self, key: u32) -> bool {
        self.xkb_keymap
            .as_ref()
            .is_some_and(|map| map.key_repeats(xkbcommon::xkb::Keycode::new(key + 8)))
    }

    pub fn key_is_held(&self, key: u32) -> bool {
        self.held_keys.contains(&key)
    }

    fn record_modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        self.mods_depressed = depressed;
        self.mods_latched = latched;
        self.mods_locked = locked;
        if let Some(xs) = self.xkb_state.as_mut() {
            xs.update_mask(depressed, latched, locked, 0, 0, group);
        }
        self.pending_input
            .push(KeyboardInput::Modifiers(KeyboardModifiers {
                depressed,
                latched,
                locked,
                group,
                effective: self.effective_modifiers(),
            }));
    }

    fn queue_key(&mut self, key: DecodedKeyEvent) {
        self.last_key_time = key.time;
        if key.state == 1 {
            // Duplicate compositor presses are not another physical gesture.
            if !self.held_keys.insert(key.keycode) {
                return;
            }
        } else {
            self.held_keys.remove(&key.keycode);
        }
        self.pending_input.push(KeyboardInput::Key {
            modifiers: self.effective_modifiers(),
            key,
            epoch: self.keyboard_epoch,
        });
    }

    fn keyboard_boundary(&mut self) {
        self.keyboard_epoch = self.keyboard_epoch.wrapping_add(1);
        tracing::debug!(target: "typio.input.keyboard", epoch = self.keyboard_epoch,
            queued = self.pending_input.len(), "keyboard boundary recorded");
        self.held_keys.clear();
        self.pending_input.push(KeyboardInput::Boundary);
    }

    fn activate(&mut self) {
        self.active = true;
        self.facts.im_focus_changed = true;
        self.keyboard_boundary();
        self.pending = SessionState {
            active: true,
            ..SessionState::default()
        };
    }

    fn deactivate(&mut self) {
        self.active = false;
        self.facts.im_focus_changed = true;
        self.keyboard_boundary();
        self.pending.active = false;
    }

    fn done(&mut self) {
        self.serial = self.serial.wrapping_add(1);
        self.initialized = true;
        self.facts.im_done_serial = self.serial;
        self.apply_pending_to_current();
    }

    /// Drain commit text staged by platform-local producers. If non-empty, the
    /// caller must pass it through [`Self::text_transaction_and_flush`].
    pub fn take_pending_commit(&mut self) -> Option<String> {
        self.composition.take_pending_commit()
    }

    /// Apply one text-input transaction to the compositor.
    ///
    /// `zwp_input_method_v2` state is double-buffered: any `commit_string` and
    /// `set_preedit_string` requests sent before one `commit(serial)` are
    /// applied atomically in protocol order.  Keeping this helper as the single
    /// commit point avoids scattering multiple same-serial commits through the
    /// keyboard path and lets the router combine "commit current segment + show
    /// remaining preedit" into one protocol transaction (ADR-0042 batch
    /// coalesce). Preedit is not held for compositor `done` — that event is a
    /// compositor state boundary, not a text-commit ack.
    pub fn text_transaction_and_flush(
        &mut self,
        commit_text: Option<&str>,
        preedit: Option<(&str, u32)>,
    ) {
        if !self.initialized || !self.active {
            return;
        }
        if commit_text.is_none() && preedit.is_none() {
            return;
        }
        if let Some(ref wayland) = self.wayland {
            if let Some(text) = commit_text {
                wayland.input_method.commit_string(text.to_string());
            }
            if let Some((text, cursor)) = preedit {
                wayland.input_method.set_preedit_string(
                    text.to_string(),
                    cursor as i32,
                    cursor as i32,
                );
            }
            wayland.input_method.commit(self.serial);
        }
    }

    /// Load an XKB keymap from a compositor-provided file descriptor.
    fn load_keymap_from_fd(&mut self, fd: std::os::fd::OwnedFd, _size: u32) {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::from(fd);
        if let Err(e) = file.seek(SeekFrom::Start(0)) {
            tracing::warn!(target: "typio.wayland.keymap", "seek to start failed: {e}");
            return;
        }

        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer) {
            tracing::warn!(target: "typio.wayland.keymap", "read keymap fd failed: {e}");
            return;
        }

        let mut keymap_string = String::from_utf8_lossy(&buffer).into_owned();
        keymap_string = keymap_string.trim_matches('\0').to_string();

        let keymap = xkbcommon::xkb::Keymap::new_from_string(
            &self.xkb_context,
            keymap_string,
            xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1,
            xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
        );

        match keymap {
            Some(km) => {
                let mut xkb_state = xkbcommon::xkb::State::new(&km);
                xkb_state.update_mask(
                    self.mods_depressed,
                    self.mods_latched,
                    self.mods_locked,
                    0,
                    0,
                    0,
                );
                self.xkb_keymap = Some(km);
                self.xkb_state = Some(xkb_state);
                tracing::debug!(target: "typio.wayland.keymap", "XKB state ready");
            }
            None => {
                tracing::warn!(target: "typio.wayland.keymap", "xkb_keymap_new_from_string failed")
            }
        }
    }
}

fn refresh_panel_after_anchor_reset(
    presentation: &mut PresentationRecord,
    schedule: &mut PanelScheduleState,
    has_candidates: bool,
) {
    presentation.invalidate();
    if has_candidates {
        schedule.mark_dirty();
    }
}

/// Wayland input-method frontend. Owns the connection, event queue,
/// and state. The caller drives the event loop via [`Self::dispatch`]
/// or [`Self::run`].
pub struct InputMethodFrontend {
    // Drop order matters here. Rust drops fields in declaration order, so
    // the panel MUST be declared before the Wayland connection: the panel
    // owns flux resources and attaches host-managed buffers to the popup
    // wl_surface. If `conn` or `state` is dropped first, teardown can touch
    // a closed connection or freed proxy. Order:
    //   1. panel     — releases flux surface/canvas/text/arena
    //   2. state     — frees wl_surface / grab proxies
    //   3. queue     — releases event queue
    //   4. conn      — closes the display socket last
    panel: Option<FluxPanel>,
    state: InputMethodState,
    queue: Option<EventQueue<InputMethodState>>,
    conn: Option<Connection>,
}

impl InputMethodFrontend {
    /// Create a headless, transport-free frontend instance for testing and mock runs.
    pub fn new_headless() -> Self {
        Self {
            panel: None,
            state: InputMethodState::new_headless(),
            queue: None,
            conn: None,
        }
    }

    /// Connect to the Wayland display, bind globals, create the
    /// input-method object, and create the lazily allocated CPU Panel.
    pub fn connect() -> Result<Self, ConnectError> {
        let mut frontend = Self::connect_internal()?;

        let surface_ptr = frontend.state.popup_surface_raw_ptr();
        let viewport = frontend.state.panel_viewport().cloned();
        let shm = frontend.state.shm().cloned();
        let qh = frontend.queue.as_ref().expect("queue present").handle();
        // Canvas and SHM buffers are allocated lazily from the first real
        // content extent. This avoids clearing and downsampling a speculative
        // 512×128 surface for every small indicator or candidate frame.
        match unsafe { FluxPanel::new_from_surface(surface_ptr, viewport, shm, qh) } {
            Ok(panel) => frontend.panel = Some(panel),
            Err(e) => tracing::warn!(target: "typio.panel.host", "FluxPanel creation failed: {e}"),
        }

        Ok(frontend)
    }

    /// Shared connection setup. The CPU Panel is created by [`Self::connect`]
    /// after the protocol state is ready; tests use this helper directly to
    /// exercise the state machine without creating flux resources.
    fn connect_internal() -> Result<Self, ConnectError> {
        let conn = Connection::connect_to_env().map_err(ConnectError::ConnectionFailed)?;
        let (globals, queue) =
            registry_queue_init::<InputMethodState>(&conn).map_err(ConnectError::RegistryFailed)?;
        let qh = queue.handle();

        // Log what the compositor actually advertises for the input-method
        // protocol family. This is the ground truth for "does this
        // compositor support IM at all, and at what version" — it answers
        // the recurring "is it us or the compositor?" question at startup.
        // The registry roundtrip inside `registry_queue_init` has already
        // populated the list, so no extra roundtrip is needed.
        globals.contents().with_list(|list: &[Global]| {
            const IM_RELEVANT: &[&str] = &[
                "wl_seat",
                "wl_compositor",
                "wl_shm",
                "wp_viewporter",
                "zwp_input_method_manager_v2",
                "zwp_virtual_keyboard_manager_v1",
                "zwp_text_input_v3",
                "zwp_text_input_manager_v3",
                "zwp_input_method_v1",
                "wp_fractional_scale_manager_v1",
                "wp_presentation",
                "xdg_wm_base",
                "ext_foreign_toplevel_list_v1",
                "zwlr_foreign_toplevel_manager_v1",
            ];
            let advertised: Vec<&Global> = list
                .iter()
                .filter(|g| IM_RELEVANT.contains(&g.interface.as_str()))
                .collect();
            if advertised.is_empty() {
                tracing::warn!(
                    target: "typio.wayland.frontend",
                    total_globals = list.len(),
                    "compositor advertises NO input-method-relevant globals — \
                     input-method-v2 likely unsupported here; expect bind failures"
                );
            } else {
                let summary: Vec<String> = advertised
                    .iter()
                    .map(|g| format!("{} v{}", g.interface, g.version))
                    .collect();
                tracing::info!(
                    target: "typio.wayland.frontend",
                    total_globals = list.len(),
                    advertised = summary.join(", "),
                    "compositor global registry snapshot (input-method-relevant interfaces)"
                );
            }
        });

        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=9, ())
            .map_err(|e| ConnectError::BindFailed("wl_seat", format!("{e:?}")))?;
        tracing::debug!(
            target: "typio.wayland.frontend",
            version = seat.version(),
            "bound wl_seat"
        );

        let im_manager: ZwpInputMethodManagerV2 = globals.bind(&qh, 1..=1, ()).map_err(|e| {
            ConnectError::BindFailed("zwp_input_method_manager_v2", format!("{e:?}"))
        })?;
        tracing::info!(
            target: "typio.wayland.frontend",
            version = im_manager.version(),
            "bound zwp_input_method_manager_v2 — input-method protocol active"
        );

        let _vk_manager: ZwpVirtualKeyboardManagerV1 =
            globals.bind(&qh, 1..=1, ()).map_err(|e| {
                ConnectError::BindFailed("zwp_virtual_keyboard_manager_v1", format!("{e:?}"))
            })?;
        tracing::debug!(
            target: "typio.wayland.frontend",
            version = _vk_manager.version(),
            "bound zwp_virtual_keyboard_manager_v1"
        );

        // Bind wl_compositor (for creating panel surfaces).
        let compositor: WlCompositor = globals
            .bind(&qh, 1..=6, ())
            .map_err(|e| ConnectError::BindFailed("wl_compositor", format!("{e:?}")))?;
        tracing::debug!(
            target: "typio.wayland.frontend",
            version = compositor.version(),
            "bound wl_compositor"
        );

        // Bind wp_viewporter if the compositor advertises it. Quantized sizing
        // with shrink hysteresis uses the viewport to avoid small resize churn.
        // Without it, content changes require exact framebuffer/SHM dimensions.
        let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
        match &viewporter {
            Some(_) => tracing::info!(
                target: "typio.wayland.viewporter",
                "compositor advertises wp_viewporter (bounded quantized Panel sizing active)"
            ),
            None => tracing::warn!(
                target: "typio.wayland.viewporter",
                "compositor lacks wp_viewporter — candidate-page size changes resize the CPU framebuffer; see ADR-0044"
            ),
        }

        // Bind wl_shm for the CPU-rendered panel path. The pool creates
        // ARGB8888 wl_buffers from anonymous shared memory; flux renders to a
        // CPU canvas and the host attaches these buffers directly.
        let shm: Option<wl_shm::WlShm> = globals.bind(&qh, 1..=1, ()).ok();
        match &shm {
            Some(_) => tracing::info!(
                target: "typio.wayland.shm",
                "compositor advertises wl_shm (offscreen-render panel path active)"
            ),
            None => tracing::warn!(
                target: "typio.wayland.shm",
                "compositor lacks wl_shm — offscreen panel path unavailable, panel will not render"
            ),
        }

        // Create a wl_surface for the panel popup.
        let popup_surface_obj = compositor.create_surface(&qh, ());

        // Attach a wp_viewport to the popup surface (if we have a
        // viewporter). Cloned into FluxPanel later so it owns its own
        // reference; the original stays here for lifetime.
        let panel_viewport: Option<WpViewport> = viewporter
            .as_ref()
            .map(|vp| vp.get_viewport(&popup_surface_obj, &qh, ()));

        // Fractional-scale and viewporter work as a pair: the former selects
        // the physical render density and the latter keeps the popup's
        // surface-local size in logical pixels.
        let fractional_scale_manager: Option<WpFractionalScaleManagerV1> =
            globals.bind(&qh, 1..=1, ()).ok();
        let panel_fractional_scale = match (
            fractional_scale_manager.as_ref(),
            panel_viewport.as_ref(),
        ) {
            (Some(manager), Some(_)) => {
                tracing::info!(
                    target: "typio.wayland.scale",
                    "fractional output scaling active for the candidate panel"
                );
                Some(manager.get_fractional_scale(&popup_surface_obj, &qh, ()))
            }
            (Some(_), None) => {
                tracing::warn!(
                    target: "typio.wayland.scale",
                    "compositor advertises fractional scale without viewporter; using integer surface scale"
                );
                None
            }
            (None, _) => {
                tracing::debug!(
                    target: "typio.wayland.scale",
                    "compositor lacks fractional scale; using integer surface scale"
                );
                None
            }
        };

        let input_method = im_manager.get_input_method(&seat, &qh, ());

        // Keyboard grab is created lazily by the focus controller when an
        // input context is focused, not eagerly here.  An eager grab would
        // capture the keypress used to launch the daemon (e.g. Enter in a
        // terminal) and forward it back to the terminal via the virtual
        // keyboard.
        let keyboard_grab: Option<ZwpInputMethodKeyboardGrabV2> = None;

        // Create the virtual keyboard for forwarding unhandled keys.
        let virtual_keyboard = _vk_manager.create_virtual_keyboard(&seat, &qh, ());

        // Create the popup surface (for the candidate panel).
        let popup_surface = input_method.get_input_popup_surface(&popup_surface_obj, &qh, ());

        let wayland = WaylandObjects {
            seat,
            input_method,
            keyboard_grab,
            virtual_keyboard,
            compositor,
            popup_surface_obj,
            popup_surface,
            viewporter,
            panel_viewport,
            fractional_scale_manager,
            panel_fractional_scale,
            shm,
        };

        let state = InputMethodState {
            wayland: Some(wayland),
            panel_presentation: PresentationRecord::default(),
            text_input_rect: None,
            composition: CompositionState::default(),
            serial: 0,
            active: false,
            initialized: false,
            xkb_context: xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS),
            xkb_state: None,
            xkb_keymap: None,
            mods_depressed: 0,
            mods_latched: 0,
            mods_locked: 0,
            pending_input: Vec::new(),
            keyboard_epoch: 1,
            last_key_time: 0,
            forwarded_keys: BTreeSet::new(),
            held_keys: BTreeSet::new(),
            #[cfg(test)]
            keyboard_output: Vec::new(),
            facts: InputFacts::default(),
            stopped: false,
            pending: SessionState::default(),
            current: SessionState::default(),
            keymap_received_this_epoch: false,
            panel_schedule_state: PanelScheduleState::default(),
            panel_coord: PanelCoordinator::new(),
            buffer_scale: 1.0,
            compositor_repeat_info: None,
            wayland_pending: crate::wayland_pending::PendingRequestTracker::default(),
        };

        Ok(Self {
            conn: Some(conn),
            queue: Some(queue),
            state,
            panel: None,
        })
    }

    #[cfg(test)]
    fn connect_test() -> Result<Self, ConnectError> {
        Self::connect_internal().or_else(|_| Ok(Self::new_headless()))
    }

    /// Immutable access to the state (serial, active flag, etc.).
    pub fn state(&self) -> &InputMethodState {
        &self.state
    }

    /// Mutable access to the state.
    pub fn state_mut(&mut self) -> &mut InputMethodState {
        &mut self.state
    }

    /// Mutable access to the candidate panel, if one was created.
    pub fn panel_mut(&mut self) -> Option<&mut FluxPanel> {
        self.panel.as_mut()
    }

    /// True if the compositor declared the input method unavailable.
    pub fn stopped(&self) -> bool {
        self.state.stopped()
    }

    /// Whether a keyboard grab object currently exists.
    pub fn keyboard_grab_present(&self) -> bool {
        self.state.keyboard_grab_present()
    }

    /// Create a new keyboard grab object.
    pub fn create_keyboard_grab(&mut self) {
        if let Some(ref queue) = self.queue {
            let qh = queue.handle();
            self.state.create_keyboard_grab(&qh);
        }
    }

    /// Destroy the current keyboard grab object.
    pub fn destroy_keyboard_grab(&mut self) {
        self.state.destroy_keyboard_grab();
    }

    /// True if the first keymap for the current grab epoch has arrived.
    pub fn keymap_received_this_epoch(&self) -> bool {
        self.state.keymap_received_this_epoch
    }

    /// The Wayland connection's file descriptor for external event loops.
    pub fn fd(&self) -> i32 {
        self.queue
            .as_ref()
            .map(|q| q.as_fd().as_raw_fd())
            .unwrap_or(-1)
    }

    /// Non-blocking dispatch of pending Wayland events.
    pub fn dispatch(&mut self) -> io::Result<()> {
        if let Some(ref mut queue) = self.queue {
            queue
                .dispatch_pending(&mut self.state)
                .map_err(|e| io::Error::other(format!("dispatch: {e}")))?;
        }
        Ok(())
    }

    /// Flush pending Wayland requests to the compositor.
    pub fn flush(&self) -> io::Result<()> {
        if let Some(ref conn) = self.conn {
            conn.flush()
                .map_err(|e| io::Error::other(format!("flush: {e}")))?;
        }
        Ok(())
    }

    /// Prepare a read from the Wayland socket, dispatching any already-queued
    /// events first. Mirrors `wl_display_prepare_read` + `dispatch_pending` in
    /// the C event loop.
    pub fn prepare_read_loop(&mut self) -> io::Result<(ReadEventsGuard, bool)> {
        let Some(ref mut queue) = self.queue else {
            return Err(io::Error::other("no live wayland queue in headless mode"));
        };
        let mut dispatched_any = false;
        loop {
            match queue.prepare_read() {
                Some(guard) => return Ok((guard, dispatched_any)),
                None => {
                    let count = queue
                        .dispatch_pending(&mut self.state)
                        .map_err(|e| io::Error::other(format!("dispatch: {e}")))?;
                    if count > 0 {
                        dispatched_any = true;
                    }
                }
            }
        }
    }

    /// Read events from the Wayland socket and dispatch any pending events
    /// that arrive. Consumes the read guard returned by `prepare_read_loop`.
    pub fn read_and_dispatch(&mut self, guard: ReadEventsGuard) -> io::Result<()> {
        let Some(ref mut queue) = self.queue else {
            return Err(io::Error::other("no live wayland queue in headless mode"));
        };
        guard
            .read()
            .map_err(|e| io::Error::other(format!("read: {e}")))?;
        queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| io::Error::other(format!("dispatch: {e}")))?;
        Ok(())
    }

    /// Blocking event loop. Runs until the connection drops.
    /// Automatically flushes pending commit text after each dispatch.
    pub fn run(&mut self) -> io::Result<()> {
        let Some(ref conn) = self.conn else {
            return Ok(());
        };
        let Some(ref mut queue) = self.queue else {
            return Ok(());
        };
        loop {
            conn.flush()
                .map_err(|e| io::Error::other(format!("flush: {e}")))?;

            queue
                .dispatch_pending(&mut self.state)
                .map_err(|e| io::Error::other(format!("dispatch: {e}")))?;

            // Flush any pending commit text to the compositor.
            if let Some(text) = self.state.take_pending_commit() {
                self.state.text_transaction_and_flush(Some(&text), None);
            }

            if let Some(read_guard) = queue.prepare_read() {
                let fd = queue.as_fd().as_raw_fd();
                let mut pollfd = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let rc = unsafe { libc::poll(&mut pollfd, 1, -1) };
                if rc < 0 {
                    let e = io::Error::last_os_error();
                    if e.raw_os_error() == Some(libc::EINTR) {
                        continue;
                    }
                    return Err(e);
                }
                if pollfd.revents & libc::POLLIN != 0 {
                    read_guard
                        .read()
                        .map_err(|e| io::Error::other(format!("read: {e}")))?;
                }
                if pollfd.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                    return Err(io::Error::other("display fd closed"));
                }
            } else {
                continue;
            }
        }
    }
}

/// Errors that can occur during [`InputMethodFrontend::connect`].
#[derive(Debug)]
pub enum ConnectError {
    ConnectionFailed(wayland_client::ConnectError),
    RegistryFailed(wayland_client::globals::GlobalError),
    BindFailed(&'static str, String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::ConnectionFailed(e) => write!(f, "Wayland connection failed: {e}"),
            ConnectError::RegistryFailed(e) => write!(f, "registry roundtrip failed: {e}"),
            ConnectError::BindFailed(iface, detail) => {
                write!(f, "cannot bind {iface}: {detail}")
            }
        }
    }
}

impl std::error::Error for ConnectError {}

// ── Dispatch impls ───────────────────────────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        _event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpInputMethodManagerV2, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpInputMethodManagerV2,
        _event: <ZwpInputMethodManagerV2 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpVirtualKeyboardManagerV1,
        _event: <ZwpVirtualKeyboardManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl InputMethodState {
    /// Apply the pending `done` batch to the current session state.
    fn apply_pending_to_current(&mut self) {
        self.current = self.pending.clone();
    }
}

impl Dispatch<ZwpInputMethodV2, ()> for InputMethodState {
    fn event(
        state: &mut Self,
        _proxy: &ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use zwp_input_method_v2::Event;
        match event {
            Event::Activate => {
                tracing::debug!(target: "typio.wayland.frontend", "Activate");
                state.activate();
            }
            Event::Deactivate => {
                tracing::debug!(target: "typio.wayland.frontend", "Deactivate");
                state.deactivate();
            }
            Event::SurroundingText {
                text,
                cursor,
                anchor,
            } => {
                state.pending.surrounding_text = Some(text);
                state.pending.cursor = cursor;
                state.pending.anchor = anchor;
            }
            Event::TextChangeCause { cause } => {
                state.pending.text_change_cause = u32::from(cause);
            }
            Event::ContentType { hint, purpose } => {
                let hint_raw: u32 = match &hint {
                    wayland_client::WEnum::Value(v) => (*v).into(),
                    wayland_client::WEnum::Unknown(u) => *u,
                };
                let purpose_raw: u32 = match &purpose {
                    wayland_client::WEnum::Value(v) => (*v).into(),
                    wayland_client::WEnum::Unknown(u) => *u,
                };
                state.pending.content_hint = hint_raw;
                state.pending.content_purpose = purpose_raw;
            }
            Event::Done => state.done(),
            Event::Unavailable => {
                state.stopped = true;
            }
        }
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpVirtualKeyboardV1,
        _event: zwp_virtual_keyboard_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCompositor, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &WlCompositor,
        _event: <WlCompositor as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewporter, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &WpViewporter,
        _event: <WpViewporter as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &WpViewport,
        _event: <WpViewport as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &WpFractionalScaleManagerV1,
        _event: <WpFractionalScaleManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for InputMethodState {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event;
        if let Some(new_scale) = fractional_scale_factor(scale) {
            state.update_panel_scale(new_scale);
        }
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for InputMethodState {
    fn event(
        state: &mut Self,
        proxy: &wl_surface::WlSurface,
        event: <wl_surface::WlSurface as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_surface::Event;
        if let Event::PreferredBufferScale { factor } = event {
            // Fractional-scale requires wl_surface.buffer_scale to remain 1
            // and provides a more precise render density than this integer
            // core event.
            let has_fractional = state
                .wayland
                .as_ref()
                .and_then(|w| w.panel_fractional_scale.as_ref())
                .is_some();
            if !has_fractional {
                state.update_panel_scale(factor as f32);
                proxy.set_buffer_scale(factor);
            }
        }
    }
}

fn fractional_scale_factor(scale_120: u32) -> Option<f32> {
    (scale_120 > 0).then_some(scale_120 as f32 / 120.0)
}

impl Dispatch<ZwpInputPopupSurfaceV2, ()> for InputMethodState {
    fn event(
        state: &mut Self,
        _proxy: &ZwpInputPopupSurfaceV2,
        event: zwp_input_popup_surface_v2::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let zwp_input_popup_surface_v2::Event::TextInputRectangle {
            x,
            y,
            width,
            height,
        } = event;
        state.text_input_rect = Some((x, y, width, height));
        state.panel_coord.note_caret_rect();
        state.panel_coord.mark_anchor_ready();
        state.wayland_pending.note_rect_received(Instant::now());
    }
}

impl Dispatch<ZwpInputMethodKeyboardGrabV2, ()> for InputMethodState {
    fn event(
        state: &mut Self,
        proxy: &ZwpInputMethodKeyboardGrabV2,
        event: zwp_input_method_keyboard_grab_v2::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use zwp_input_method_keyboard_grab_v2::Event;
        if state
            .wayland
            .as_ref()
            .and_then(|w| w.keyboard_grab.as_ref())
            != Some(proxy)
        {
            return;
        }
        match event {
            Event::Keymap { format, fd, size } => {
                tracing::debug!(
                    target: "typio.wayland.keymap",
                    "Keymap event received, format={format:?} size={size}"
                );
                state.release_forwarded_keys();
                state.pending_input.clear();
                state.keyboard_boundary();
                state.keymap_received_this_epoch = false;
                state.wayland_pending.note_keymap_received(Instant::now());
                let fmt_raw: u32 = match &format {
                    wayland_client::WEnum::Value(v) => *v as u32,
                    wayland_client::WEnum::Unknown(u) => *u,
                };
                if fmt_raw != 1 {
                    return;
                }
                // Forward the keymap to the virtual keyboard before any
                // `key`/`modifiers` requests. The compositor rejects those
                // with protocol error 0 (no_keymap) if the vk has no keymap.
                // `load_keymap_from_fd` consumes the fd, so dup it first.
                // The wayland backend dups the fd again when serializing the
                // request, so it is safe to drop `vk_fd` right after the call.
                if let Some(ref wayland) = state.wayland {
                    match fd.try_clone() {
                        Ok(vk_fd) => wayland
                            .virtual_keyboard
                            .keymap(fmt_raw, vk_fd.as_fd(), size),
                        Err(e) => {
                            tracing::warn!(target: "typio.wayland.keymap", "dup keymap fd for vk failed: {e}");
                            return;
                        }
                    }
                }
                state.xkb_state = None;
                state.xkb_keymap = None;
                state.mods_depressed = 0;
                state.mods_latched = 0;
                state.mods_locked = 0;
                state.load_keymap_from_fd(fd, size);
                state.keymap_received_this_epoch = state.xkb_state.is_some();
            }
            Event::Key {
                time,
                key,
                state: key_state,
                serial: _,
            } => {
                let raw_state: u32 = match &key_state {
                    wayland_client::WEnum::Value(v) => *v as u32,
                    wayland_client::WEnum::Unknown(u) => *u,
                };

                let xkb_keycode = key + 8;
                let kc = xkbcommon::xkb::Keycode::new(xkb_keycode);
                let key_direction = if raw_state == 1 {
                    xkbcommon::xkb::KeyDirection::Down
                } else {
                    xkbcommon::xkb::KeyDirection::Up
                };
                if let Some(ref mut xs) = state.xkb_state {
                    xs.update_key(kc, key_direction);
                }

                let keysym: u32 = state
                    .xkb_state
                    .as_ref()
                    .map_or(0, |s| s.key_get_one_sym(kc).into());
                let unicode = state
                    .xkb_state
                    .as_ref()
                    .map_or(String::new(), |s| s.key_get_utf8(kc));

                state.queue_key(DecodedKeyEvent {
                    keycode: key,
                    xkb_keycode,
                    keysym,
                    unicode,
                    state: raw_state,
                    time,
                });
            }
            Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                serial: _,
            } => {
                state.record_modifiers(mods_depressed, mods_latched, mods_locked, group);
            }
            Event::RepeatInfo { rate, delay } => {
                state.compositor_repeat_info = Some((rate, delay));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_error_display_is_human_readable() {
        let e = ConnectError::BindFailed("zwp_input_method_manager_v2", "NotPresent".to_string());
        let s = format!("{e}");
        assert!(s.contains("zwp_input_method_manager_v2"));
    }

    #[test]
    fn fractional_scale_wire_value_converts_from_120ths() {
        assert_eq!(fractional_scale_factor(0), None);
        assert_eq!(fractional_scale_factor(120), Some(1.0));
        assert_eq!(fractional_scale_factor(150), Some(1.25));
        assert_eq!(fractional_scale_factor(240), Some(2.0));
    }

    #[test]
    fn composition_seq_changes_only_for_visual_candidate_changes() {
        let mut composition = CompositionState::default();
        let seq1 = composition.set_candidates(vec!["alpha".to_string(), "beta".to_string()], 1);
        let seq2 = composition.set_candidates(vec!["alpha".to_string(), "beta".to_string()], 1);
        let seq3 = composition.set_candidates(vec!["alpha".to_string(), "beta".to_string()], 0);

        assert_ne!(seq1, 0);
        assert_eq!(seq2, seq1);
        assert_ne!(seq3, seq2);
    }

    #[test]
    fn anchor_reset_requeues_an_unchanged_candidate_snapshot() {
        let mut presentation = PresentationRecord::default();
        let mut schedule = PanelScheduleState::Idle;
        presentation.mark_presented(7);

        refresh_panel_after_anchor_reset(&mut presentation, &mut schedule, true);

        assert!(!presentation.is_current(7));
        assert_eq!(schedule, PanelScheduleState::Dirty);
    }

    #[test]
    fn anchor_reset_does_not_schedule_an_empty_candidate_panel() {
        let mut presentation = PresentationRecord::default();
        let mut schedule = PanelScheduleState::Idle;
        presentation.mark_presented(7);

        refresh_panel_after_anchor_reset(&mut presentation, &mut schedule, false);

        assert!(!presentation.is_current(7));
        assert_eq!(schedule, PanelScheduleState::Idle);
    }

    #[test]
    fn state_helpers_round_trip() {
        let mut frontend = InputMethodFrontend::connect_test().expect("connect_test");

        let state = frontend.state_mut();
        assert_eq!(state.serial(), 0);
        assert!(!state.is_active());
        assert!(!state.stopped());

        state.set_candidates(vec!["alpha".to_string(), "beta".to_string()], 1);
        assert_eq!(state.composition.candidates, vec!["alpha", "beta"]);
        assert_eq!(state.composition.selected_candidate, 1);
        assert!(!state.panel_presentation_current(state.composition.composition_seq));
        state.mark_panel_presented(state.composition.composition_seq);
        assert!(state.panel_presentation_current(state.composition.composition_seq));
        state.invalidate_panel_presentation();
        assert!(!state.panel_presentation_current(state.composition.composition_seq));

        state.mark_panel_dirty();
        assert_eq!(state.panel_schedule_state, PanelScheduleState::Dirty);

        state.clear_panel_state();
        assert!(state.composition.candidates.is_empty());
        assert_eq!(state.composition.selected_candidate, 0);
        assert_eq!(state.panel_schedule_state, PanelScheduleState::Idle);

        state.facts.im_done_serial = 7;
        let facts = state.take_facts();
        assert_eq!(facts.im_done_serial, 7);
        assert_eq!(state.facts.im_done_serial, 0);

        let press = DecodedKeyEvent {
            keycode: 30,
            xkb_keycode: 38,
            keysym: 0x0061,
            unicode: "a".to_string(),
            state: 1,
            time: 123,
        };
        let release = DecodedKeyEvent {
            keycode: 30,
            xkb_keycode: 38,
            keysym: 0x0061,
            unicode: String::new(),
            state: 0,
            time: 130,
        };

        state.queue_key(press.clone());
        state.queue_key(release.clone());
        let keys: Vec<_> = state
            .take_pending_input()
            .into_iter()
            .filter_map(|input| {
                if let KeyboardInput::Key { key, .. } = input {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(keys, vec![press, release]);
        assert!(state.take_pending_input().is_empty());

        state.composition.set_pending_commit("hello".to_string());
        assert_eq!(state.take_pending_commit(), Some("hello".to_string()));
        assert!(state.take_pending_commit().is_none());
    }

    #[test]
    fn headless_frontend_operations() {
        let mut frontend = InputMethodFrontend::new_headless();
        assert_eq!(frontend.fd(), -1);
        assert!(frontend.dispatch().is_ok());
        assert!(frontend.flush().is_ok());
        assert!(!frontend.keyboard_grab_present());
        frontend.create_keyboard_grab();
        assert!(!frontend.keyboard_grab_present());
        frontend.destroy_keyboard_grab();
        assert_eq!(
            frontend.state().popup_surface_raw_ptr(),
            core::ptr::null_mut()
        );
        assert!(frontend.state().shm().is_none());
        assert!(frontend.state().panel_viewport().is_none());

        let state = frontend.state_mut();
        state.forward_key(0, 30, 1);
        state.forward_modifiers(KeyboardModifiers::default());
        state.commit_protocol_state();
        state.text_transaction_and_flush(Some("test"), None);
    }
    fn keyboard_state() -> InputMethodState {
        let mut state = InputMethodState::new_headless();
        state.keymap_received_this_epoch = true;
        state
    }

    fn letter(state: u32) -> DecodedKeyEvent {
        DecodedKeyEvent {
            keycode: 17,
            xkb_keycode: 25,
            keysym: 0x77,
            unicode: "w".into(),
            state,
            time: 100 + state,
        }
    }

    // Exercise the same transport preparation used by the daemon. The engine
    // declines keys in this fixture, so every routable press is forwarded.
    fn drain_keyboard(state: &mut InputMethodState) {
        for input in state.take_pending_input() {
            if let Some(KeyboardInput::Key { key, .. }) = state.prepare_input(input) {
                state.forward_key(key.time, key.keycode, key.state);
            }
        }
    }

    #[test]
    fn text_only_done_cannot_erase_or_replay_a_focus_boundary() {
        let mut state = keyboard_state();
        state.deactivate();
        state.done();
        state.activate();
        state.done();
        state.done();
        let facts = state.take_facts();
        assert!(facts.im_focus_changed);
        assert!(facts.im_is_active);
        assert_eq!(facts.im_done_serial, 3);
        state.done();
        assert!(!state.take_facts().im_focus_changed);
        state.deactivate();
        assert!(state.take_facts().im_focus_changed);
        state.done();
        assert!(!state.take_facts().im_focus_changed);
    }

    #[test]
    fn last_focus_event_wins_in_both_orders() {
        let mut state = keyboard_state();
        state.activate();
        state.deactivate();
        state.done();
        let facts = state.take_facts();
        assert!(facts.im_focus_changed);
        assert!(!facts.im_is_active);
        state.deactivate();
        state.activate();
        state.done();
        let facts = state.take_facts();
        assert!(facts.im_focus_changed);
        assert!(facts.im_is_active);
    }

    #[test]
    fn modifier_samples_and_keys_keep_arrival_order() {
        let mut state = keyboard_state();
        state.activate();
        state.done();
        drain_keyboard(&mut state);
        state.record_modifiers(4, 0, 0, 0); // default xkb Control wire bit
        state.queue_key(letter(1));
        state.record_modifiers(0, 0, 0, 0);
        state.queue_key(letter(0));
        assert!(
            state.keyboard_output.is_empty(),
            "callbacks must not emit modifiers ahead of keys"
        );
        let input = state.take_pending_input();
        assert!(
            matches!(&input[1], KeyboardInput::Key { modifiers, .. } if *modifiers == Modifiers::CTRL)
        );
        for event in input {
            if let Some(KeyboardInput::Key { key, .. }) = state.prepare_input(event) {
                state.forward_key(key.time, key.keycode, key.state);
            }
        }
        assert!(
            matches!(&state.keyboard_output[0], KeyboardInput::Modifiers(m) if m.depressed == 4)
        );
        assert!(
            matches!(&state.keyboard_output[1], KeyboardInput::Key { key, .. } if key.state == 1)
        );
        assert!(
            matches!(&state.keyboard_output[2], KeyboardInput::Modifiers(m) if m.depressed == 0)
        );
        assert!(
            matches!(&state.keyboard_output[3], KeyboardInput::Key { key, .. } if key.state == 0)
        );
    }

    #[test]
    fn inactive_press_and_active_release_share_one_ledger() {
        let mut state = keyboard_state();
        state.queue_key(letter(1));
        drain_keyboard(&mut state);
        assert!(state.key_is_forwarded(17));
        state.activate();
        state.done();
        state.queue_key(letter(0));
        drain_keyboard(&mut state);
        assert!(!state.key_is_forwarded(17));
        let keys: Vec<_> = state
            .keyboard_output
            .iter()
            .filter_map(|event| {
                if let KeyboardInput::Key { key, .. } = event {
                    Some(key.state)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            keys,
            [1, 0],
            "synthetic and physical releases must not duplicate"
        );
        state.deactivate();
        state.queue_key(letter(1));
        drain_keyboard(&mut state);
        assert!(
            state.key_is_forwarded(17),
            "fresh press must not inherit a release marker"
        );
        state.queue_key(letter(0));
        drain_keyboard(&mut state);
        assert!(!state.key_is_forwarded(17));
    }

    #[test]
    fn grab_teardown_releases_forwarded_keys_and_fences_queued_presses() {
        let mut state = keyboard_state();
        state.queue_key(letter(1));
        drain_keyboard(&mut state);
        state.destroy_keyboard_grab();
        state.queue_key(letter(1));
        drain_keyboard(&mut state);
        assert!(state.forwarded_keys.is_empty());
        assert!(!state.keymap_received_this_epoch);
    }

    #[test]
    fn shortcut_handoff_is_paired_for_every_dispatch_partition() {
        // Every possible placement of reactor drains in one physical Ctrl+W
        // gesture. Covers a queued release before a boundary, two done events
        // before observation, inactive release, and one-event dispatches.
        for release_before_boundary in [false, true] {
            for partition in 0..(1u32 << 10) {
                let mut state = keyboard_state();
                state.activate();
                state.done();
                drain_keyboard(&mut state);
                state.take_facts();
                for step in 0..11 {
                    match step {
                        0 => state.record_modifiers(4, 0, 0, 0),
                        1 => state.queue_key(letter(1)),
                        2 if release_before_boundary => state.queue_key(letter(0)),
                        3 => state.deactivate(),
                        4 => state.done(),
                        5 => state.activate(),
                        6 | 7 | 10 => state.done(),
                        8 => state.record_modifiers(0, 0, 0, 0),
                        9 if !release_before_boundary => state.queue_key(letter(0)),
                        _ => {}
                    }
                    if partition & (1 << step) != 0 {
                        drain_keyboard(&mut state);
                        state.take_facts();
                    }
                }
                drain_keyboard(&mut state);
                assert!(state.forwarded_keys.is_empty(), "partition {partition}");
                assert!(!state.key_is_held(17));
                let mut held = false;
                let mut modifiers = 0;
                for event in state.keyboard_output {
                    match event {
                        KeyboardInput::Modifiers(sample) => modifiers = sample.depressed,
                        KeyboardInput::Key { key, .. } => {
                            if key.state == 1 {
                                assert!(!held, "duplicate press in partition {partition}");
                                assert_eq!(
                                    modifiers, 4,
                                    "shortcut lost Ctrl in partition {partition}"
                                );
                                held = true;
                            } else {
                                assert!(held, "orphan release in partition {partition}");
                                held = false;
                            }
                        }
                        KeyboardInput::Boundary => unreachable!(),
                    }
                }
                assert!(!held, "stuck w in partition {partition}");
            }
        }
    }
}
