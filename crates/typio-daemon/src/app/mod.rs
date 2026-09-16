//! Top-level daemon lifecycle.
//!
//! Owns the `TypioInstance`, wires engine loading, signal handling,
//! restart-on-exec, and the Wayland frontend / tray / IPC surfaces.

mod cli;
mod event_channel;
mod event_loop;
pub(crate) mod font_config;
mod indicator;
mod input_driver;
#[cfg(feature = "wayland")]
mod one_shot_timer;
mod panel_driver;
mod reactor;
mod signals;
mod tray;

#[cfg(feature = "systray")]
use tray::{build_tray_snapshot, install_tray_action_handler, update_tray_from_controller};

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::path::PathBuf;
use std::rc::Rc;

use clap::Parser;
use typio_runtime::instance::TypioInstance;

pub use cli::AppOptions;
use cli::Cli;

use crate::config_watcher::ConfigWatcher;
use crate::engine_loader::resolve_engine_dirs;
use crate::indicator::{Indicator, IndicatorConfig};
use crate::ipc::protocol;
use crate::ipc::protocol::topics;
use crate::ipc_bus::{IpcBus, TypioBackend, TypioRegistryView};
use crate::resume_signal::ResumeSignal;
use crate::session_glue::FocusDriver;
use crate::state_controller::{StateChange, StateController};
#[cfg(feature = "systray")]
use crate::tray_sni::Tray;
use crate::uds_server::UdsServer;

#[cfg(feature = "wayland")]
use crate::input_method::InputMethodFrontend;
#[cfg(feature = "wayland")]
use crate::keyboard::router::KeyboardRouter;
#[cfg(feature = "wayland")]
use crate::repeat_timer::{self, RepeatTimer};
use event_channel::{DaemonEventSender, ReactorWaker};
#[cfg(feature = "wayland")]
use one_shot_timer::OneShotTimer;

/// Cross-thread events delivered to the main loop.
///
/// Senders live in:
/// - the IPC stop callback (UDS `daemon.stop` method),
/// - the StatusNotifierItem tray action callback (zbus internal thread).
///
/// The receiver is owned by [`App`] and drained once per reactor step by the
/// main loop. A paired eventfd wakes an idle poll for every successful send.
/// This keeps every mutation of `App` state on the event-loop thread — the
/// alternative (`AtomicBool` flags for each cause) loses type information and
/// forces the loop to do untyped "refresh everything" work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonEvent {
    /// Cleanly stop the daemon. Causes the main loop to exit.
    Shutdown,
    /// Stop and re-exec with the same argv. Causes the main loop to
    /// exit; [`App::finish`] then `execv`s.
    Restart,
    /// Runtime state changed (engine / language / voice switch from
    /// the tray). Re-sync `StateController`, IPC bus, and tray surface.
    StateRefresh,
    /// A tray action forwarded from the zbus worker for main-loop execution.
    #[cfg(feature = "systray")]
    TrayAction(crate::tray_sni::TrayAction),
}

/// The running daemon.
pub struct App {
    argv: Vec<CString>,
    options: AppOptions,
    instance: Option<crate::runtime::SharedInstance>,
    state_controller: Option<StateController<TypioRegistryView>>,
    ipc_bus: Option<Rc<RefCell<IpcBus>>>,
    #[cfg(feature = "systray")]
    tray: Option<Tray>,
    #[cfg(feature = "wayland")]
    frontend: Option<InputMethodFrontend>,
    #[cfg(feature = "wayland")]
    router: Option<KeyboardRouter>,
    #[cfg(feature = "wayland")]
    repeat_timer: Option<RepeatTimer>,
    #[cfg(feature = "wayland")]
    resume_signal: Option<ResumeSignal>,
    #[cfg(feature = "wayland")]
    focus_driver: Option<FocusDriver>,
    /// Voice push-to-talk controller: owns the runtime voice session and
    /// the PipeWire (`pw-record`) audio source. `None` if session creation
    /// failed at startup.
    #[cfg(feature = "wayland")]
    voice: Option<crate::voice::VoiceController>,
    /// On-screen indicator state machine (gate state + label composition).
    /// Pure; the popup surface is owned by `PanelCoordinator`, the auto-hide
    /// timer by [`Self::indicator_timer`].
    indicator: Option<Indicator>,
    /// Cached indicator configuration snapshot. Re-read from typio-runtime on
    /// startup and on every config reload so the running loop never does
    /// FFI on the hot path.
    indicator_config: IndicatorConfig,
    /// Cached panel font configuration snapshot (family + size). Re-read on
    /// startup and reload, then applied to the candidate panel's
    /// [`TextRaster`](crate::text_raster::TextRaster) so its primary-family
    /// and per-size caches reflect the user's `display.font_*` settings.
    panel_font_config: font_config::PanelFontConfig,
    /// Auto-hide timerfd for the indicator. Armed when the indicator
    /// actually becomes visible (coordinator accepted the show); disarmed
    /// on hide, focus-loss, or shutdown. Polled as part of the main poll
    /// set; expiry drives `indicator.hide()` + panel detach.
    #[cfg(feature = "wayland")]
    indicator_timer: Option<OneShotTimer>,
    /// Auto-hide timerfd for the voice status banner. Separate from the
    /// keyboard/language indicator so voice status does not affect indicator
    /// recency gates or get hidden by the indicator timer.
    #[cfg(feature = "wayland")]
    voice_status_timer: Option<OneShotTimer>,
    /// Last voice session state observed by the banner driver. Used to tell
    /// a productive `Processing → Idle` (a result follows) from a barren one
    /// (nothing recognised → show no-speech feedback).
    #[cfg(feature = "wayland")]
    voice_last_state: typio_runtime::voice::types::VoiceState,
    /// Auto-hide policy for a voice banner the panel coordinator queued
    /// because the anchor was not ready. Applied when the deferred show
    /// finally flushes so a sticky banner is not downgraded to transient.
    #[cfg(feature = "wayland")]
    voice_pending_banner: Option<indicator::VoiceBanner>,
    config_watcher: Option<ConfigWatcher>,
    /// Last time the idle-worker reaper ran. The reaper itself is cheap,
    /// but it takes the instance borrow and walks every engine slot, so it
    /// is throttled to a coarse cadence instead of running per reactor step
    /// (which fires on every keystroke).
    #[cfg(feature = "wayland")]
    last_idle_reap: Option<std::time::Instant>,
    /// Sender half of the daemon event channel. Cloned into the IPC stop
    /// callback and the tray action handler; every send also writes
    /// `event_waker`.
    event_tx: DaemonEventSender,
    /// Receiver half of the daemon event channel. Drained once per reactor step
    /// by the main loop; never shared with another thread (`Receiver` is
    /// `!Sync`).
    event_rx: Option<std::sync::mpsc::Receiver<DaemonEvent>>,
    /// Pollable counterpart to `event_rx`. Every cross-thread event and
    /// handled Unix signal writes this eventfd so an idle reactor wakes
    /// without a periodic timeout.
    event_waker: ReactorWaker,
    /// Observed `DaemonEvent::Restart` during the last drain. Consumed
    /// by [`Self::finish`] to decide whether to `execv` after exit.
    saw_restart: bool,
}

fn format_engine_list(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

fn has_engine_manifest(dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(crate::engine_loader::manifest::is_manifest_filename)
    })
}

impl App {
    /// CLI verbosity selected at startup.
    pub fn verbosity(&self) -> u8 {
        self.options.verbosity
    }

    /// Parse CLI args and create an uninitialized app shell.
    pub fn from_env() -> Result<Self, String> {
        // `parse()` handles --help and --version by printing and exiting with
        // code 0, matching standard CLI conventions.
        let cli = Cli::parse();
        let options = AppOptions::from(cli);
        let argv: Vec<CString> = std::env::args()
            .map(CString::new)
            .collect::<Result<_, _>>()
            .map_err(|_| "argument contains NUL".to_string())?;
        let (event_tx, event_rx, event_waker) = event_channel::channel()
            .map_err(|error| format!("failed to create daemon eventfd: {error}"))?;
        Ok(Self {
            argv,
            options,
            instance: None,
            state_controller: None,
            ipc_bus: None,
            #[cfg(feature = "systray")]
            tray: None,
            #[cfg(feature = "wayland")]
            frontend: None,
            #[cfg(feature = "wayland")]
            router: None,
            #[cfg(feature = "wayland")]
            repeat_timer: None,
            #[cfg(feature = "wayland")]
            resume_signal: None,
            #[cfg(feature = "wayland")]
            focus_driver: None,
            #[cfg(feature = "wayland")]
            voice: None,
            indicator: None,
            indicator_config: IndicatorConfig::default(),
            panel_font_config: font_config::PanelFontConfig::default(),
            #[cfg(feature = "wayland")]
            indicator_timer: None,
            #[cfg(feature = "wayland")]
            voice_status_timer: None,
            #[cfg(feature = "wayland")]
            voice_last_state: typio_runtime::voice::types::VoiceState::Idle,
            #[cfg(feature = "wayland")]
            voice_pending_banner: None,
            config_watcher: None,
            #[cfg(feature = "wayland")]
            last_idle_reap: None,
            event_tx,
            event_rx: Some(event_rx),
            event_waker,
            saw_restart: false,
        })
    }

    /// Default config directory: `$XDG_CONFIG_HOME/typio` or `~/.config/typio`.
    fn default_config_dir() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".config")
            })
            .join("typio")
    }

    /// Initialize the Typio instance and load engines.
    pub fn init(&mut self) -> Result<(), String> {
        // Engine search path: CLI dirs > $TYPIO_ENGINE_PATH > system dir.
        let engine_dirs = resolve_engine_dirs(self.options.engine_dirs.iter().cloned());

        let mut instance = TypioInstance::new_rust(
            self.options.config_dir.as_deref(),
            self.options.data_dir.as_deref(),
            None, // state_dir — let typio-runtime pick the default.
            // Retained for host-side engine.reload manifest resolution.
            engine_dirs
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        );

        instance
            .init_rust()
            .map_err(|e| format!("TypioInstance init failed: {e:?}"))?;

        // Set up the config watcher so the event loop can react to file changes.
        let config_dir = self
            .options
            .config_dir
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(Self::default_config_dir);
        self.config_watcher = ConfigWatcher::new(&config_dir).ok();
        if let Some(ref mut watcher) = self.config_watcher {
            let engines_dir = config_dir.join("engines");
            if engines_dir.is_dir() {
                let _ = watcher.watch_engines_dir(&engines_dir);
            }
        }

        // Register engines from the resolved directories via Typio's
        // native Rust API (ADR-0035). EngineLoader handles manifest
        // discovery, parsing, capability negotiation, and ProcessBackend
        // registration in one pass. Embedding hosts can perform the same
        // operation through the same typed registry API.
        if instance.registry_rust().is_none() {
            return Err("engine registry not available".to_string());
        }

        let mut loader = crate::engine_loader::EngineLoader::with_voice();
        let mut registered_keyboards: Vec<String> = Vec::new();
        let mut registered_voices: Vec<String> = Vec::new();
        for dir in &engine_dirs {
            let dir_path = std::path::Path::new(dir);
            if !dir_path.is_dir() {
                continue;
            }
            let Some(mut reg) = instance.registry_rust_mut() else {
                continue;
            };
            let report = loader.load_dir(&mut reg, dir_path);
            if report.manifest_count == 0 {
                let build_dir = dir_path.join("build");
                if has_engine_manifest(&build_dir) {
                    tracing::warn!(
                        target: "typio.startup",
                        dir = %dir_path.display(),
                        hint = %build_dir.display(),
                        "no engine manifests found; did you mean the build/ subdirectory?"
                    );
                } else {
                    tracing::warn!(
                        target: "typio.startup",
                        dir = %dir_path.display(),
                        "no engine manifests found (expected typio-engine-*.toml)"
                    );
                }
            }
            for info in report.registered {
                let kind = match info.engine_type {
                    typio_runtime::core::engine::EngineType::Keyboard => "keyboard",
                    typio_runtime::core::engine::EngineType::Voice => "voice",
                };
                tracing::info!(
                    target: "typio.startup",
                    kind,
                    engine = %info.name,
                    dir = %dir_path.display(),
                    "registered engine"
                );
                if info.engine_type == typio_runtime::core::engine::EngineType::Voice {
                    registered_voices.push(info.name);
                } else {
                    registered_keyboards.push(info.name);
                }
            }
            for (path, reason) in &report.skipped {
                tracing::warn!(
                    target: "typio.startup",
                    path = %path.display(),
                    ?reason,
                    "skipped engine manifest"
                );
            }
            for (path, err) in &report.failed {
                tracing::warn!(
                    target: "typio.startup",
                    path = %path.display(),
                    error = %err,
                    "failed to load engine manifest"
                );
            }
        }

        // Restore the persisted language (last-used if still enabled,
        // otherwise the first enabled language). This both activates the
        // matching keyboard/voice engines for that language and sets
        // `active_language` so the tray icon shows the right badge (中 / EN /
        // あ …) instead of the generic `typio-keyboard-symbolic`. Falls back
        // to the first registered keyboard when no languages are declared so
        // legacy layout-only setups keep working.
        if instance.restore_language().is_err() {
            if let Some(first) = registered_keyboards.first() {
                if instance
                    .registry_rust_mut()
                    .is_some_and(|mut registry| registry.activate_keyboard(first).is_ok())
                {
                    tracing::info!(target: "typio.startup", keyboard = %first, "active keyboard set");
                }
            }
        } else {
            tracing::info!(target: "typio.startup", "language restored");
        }
        let active_voice = instance
            .registry_rust()
            .and_then(|registry| registry.active_voice_name().map(str::to_string));
        tracing::info!(
            target: "typio.startup",
            keyboards = %format_engine_list(&registered_keyboards),
            "registered keyboard engine(s)"
        );
        tracing::info!(
            target: "typio.startup",
            voices = %format_engine_list(&registered_voices),
            "registered voice engine(s)"
        );
        if let Some(active_voice) = active_voice {
            tracing::info!(target: "typio.startup", voice = %active_voice, "active voice set");
        } else if !registered_voices.is_empty() {
            tracing::warn!(
                target: "typio.startup",
                "registered voice engine(s), but no active voice selected"
            );
        }

        let instance = Rc::new(RefCell::new(instance));
        self.instance = Some(instance.clone());

        // Wire the mode observer so engine-internal mode switches
        // (rime schema changes, 中/A toggle, etc.) reach the indicator.
        // The trampoline's sender lives in `signals::MODE_CALLBACK_TX` so
        // the callback (which fires on the engine-comm thread for
        // out-of-process engines like rime) can safely reach the main
        // loop. Without this, only Ctrl+Shift language/engine switches
        // trigger the indicator — rime's own mode/schema switches are silent.
        let mode_tx = self.event_tx.clone();
        instance.borrow_mut().set_mode_observer(move |_| {
            let _ = mode_tx.send(DaemonEvent::StateRefresh);
        });

        #[cfg(feature = "wayland")]
        {
            match InputMethodFrontend::connect() {
                Ok(frontend) => {
                    tracing::info!(target: "typio.startup", "Wayland input-method frontend connected");
                    self.frontend = Some(frontend);
                    // Apply the initial panel font config now that the panel
                    // exists, so the first frame uses the user's font/size
                    // instead of the rasteriser defaults.
                    self.panel_font_config = self.load_display_font_config();
                    if let Some(ref mut frontend) = self.frontend {
                        if let Some(panel) = frontend.panel_mut() {
                            panel.set_font_config(self.panel_font_config.clone());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "typio.startup", error = %e, "Wayland frontend not available");
                }
            }

            if let Some(instance) = self.instance.as_ref().cloned() {
                self.router = Some(KeyboardRouter::new(instance.borrow_mut().as_mut()));

                match RepeatTimer::new() {
                    Ok(timer) => {
                        self.repeat_timer = Some(timer);
                    }
                    Err(e) => {
                        tracing::warn!(target: "typio.startup", error = %e, "failed to create repeat timer");
                    }
                }

                self.resume_signal = Some(ResumeSignal::new());
                self.focus_driver = Some(FocusDriver::new());

                // Voice push-to-talk: create the runtime voice session and
                // attach a PipeWire (`pw-record`) audio source. Capture is
                // now real; transcription additionally requires a registered
                // voice engine (gated at PTT-press time via `is_available`).
                match crate::voice::VoiceController::new(instance.clone()) {
                    Some(voice) => {
                        tracing::info!(
                            target: "typio.startup",
                            "voice session created (PipeWire pw-record capture)"
                        );
                        self.voice = Some(voice);
                    }
                    None => {
                        tracing::warn!(target: "typio.startup", "failed to create voice session")
                    }
                }

                // Indicator subsystem: state machine + auto-hide timerfd.
                // The timer is created disarmed and only armed when a show
                // actually lands on screen (see `arm_indicator_timer`).
                self.indicator = Some(Indicator::new());
                match OneShotTimer::new() {
                    Ok(tf) => self.indicator_timer = Some(tf),
                    Err(e) => {
                        tracing::warn!(target: "typio.startup", error = %e, "failed to create indicator timer")
                    }
                }
                match OneShotTimer::new() {
                    Ok(tf) => self.voice_status_timer = Some(tf),
                    Err(e) => {
                        tracing::warn!(target: "typio.startup", error = %e, "failed to create voice status timer")
                    }
                }
            }

            self.indicator_config = self.load_indicator_config();
        }

        let instance = self.instance.as_ref().expect("instance stored").clone();
        self.state_controller = Some(StateController::new(TypioRegistryView::new(
            instance.clone(),
        )));

        #[cfg(feature = "systray")]
        {
            let mut tray = Tray::new();
            let registered = tray.register();
            if registered {
                tracing::info!(
                    target: "typio.startup",
                    service = %tray.service_name(),
                    "StatusNotifierItem registered"
                );
            } else {
                tracing::warn!(
                    target: "typio.startup",
                    "tray did not register (no org.kde.StatusNotifierWatcher on the session bus?)"
                );
            }
            install_tray_action_handler(&tray, self.event_tx.clone());
            if let Some(snapshot) = build_tray_snapshot(&instance) {
                tray.set_menu_snapshot(snapshot);
            }
            self.tray = Some(tray);
        }

        Ok(())
    }

    /// Run the daemon until shutdown.
    pub fn run(&mut self) -> i32 {
        if self.instance.is_none() {
            tracing::error!(target: "typio.lifecycle", "app not initialized");
            return 1;
        }

        if let Err(error) = signals::install_signal_handlers(&self.event_waker) {
            tracing::error!(target: "typio.lifecycle", %error, "failed to install signal handlers");
            return 1;
        }

        let socket_path = self
            .options
            .socket_path
            .clone()
            .unwrap_or_else(protocol::socket_path);
        let server = match UdsServer::bind(&socket_path) {
            Ok(s) => {
                tracing::info!(target: "typio.startup", path = %socket_path.display(), "UDS listening");
                s
            }
            Err(e) => {
                tracing::warn!(target: "typio.startup", error = %e, "UDS bind failed — running without IPC");
                return self.run_without_uds();
            }
        };

        self.print_startup_banner();

        let instance = self.instance.as_ref().expect("instance stored").clone();
        let backend = TypioBackend::new(instance.clone());
        let service = crate::service::StatusService::new(backend);
        let ipc_bus = Rc::new(RefCell::new(IpcBus::new(server, service)));
        // The IPC `daemon.stop` method routes through the same event
        // channel as tray actions — sending Shutdown here makes the main
        // loop the single place that decides when to exit.
        let stop_tx = self.event_tx.clone();
        ipc_bus.borrow_mut().set_stop_callback(move || {
            let _ = stop_tx.send(DaemonEvent::Shutdown);
        });

        // IPC-driven mutations (engine/language switch, config reload, engine
        // load/unload) bypass the Rust `StateController` notification path —
        // the registry is mutated through its owned Rust API. Route a
        // `StateRefresh` back to the main loop so derived surfaces (controller
        // snapshot, tray icon, tooltip, menu) re-sync against the new state.
        // Without this, `typioctl language use en` would update the registry
        // but leave the tray badge showing the previous language.
        let state_tx = self.event_tx.clone();
        ipc_bus.borrow_mut().set_state_change_callback(move || {
            let _ = state_tx.send(DaemonEvent::StateRefresh);
        });

        if let Some(ref mut controller) = self.state_controller {
            let ipc = ipc_bus.clone();
            controller.add_listener(Box::new(move |change| {
                let (topic, payload) = match change {
                    StateChange::Engine | StateChange::VoiceEngine => {
                        (topics::ENGINE_CHANGED, serde_json::json!({}))
                    }
                    StateChange::Language => (topics::LANGUAGE_CHANGED, serde_json::json!({})),
                    _ => (topics::RUNTIME_CHANGED, serde_json::json!({})),
                };
                ipc.borrow_mut().emit(topic, &payload);
            }));
            controller.sync();

            #[cfg(feature = "systray")]
            if let Some(ref tray) = self.tray {
                update_tray_from_controller(tray, controller, &instance);
            }
        }

        self.ipc_bus = Some(ipc_bus.clone());

        tracing::info!(target: "typio.lifecycle", "running (Ctrl+C to exit)");

        #[cfg(feature = "wayland")]
        if self.frontend.is_some() && self.router.is_some() && self.repeat_timer.is_some() {
            return self.run_with_wayland(Some(&ipc_bus));
        }

        self.run_with_uds(&ipc_bus)
    }

    fn run_with_uds(&mut self, ipc_bus: &Rc<RefCell<IpcBus>>) -> i32 {
        let uds_fd = ipc_bus.borrow().epoll_fd();
        let mut pollfds = [
            libc::pollfd {
                fd: uds_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.event_waker.fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        while !self.drain_events() {
            for pollfd in &mut pollfds {
                pollfd.revents = 0;
            }
            let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, -1) };
            if rc < 0 {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                tracing::error!(target: "typio.lifecycle", error = %e, "poll failed");
                return 1;
            }
            if pollfds[1].revents & libc::POLLIN != 0 {
                continue;
            }
            if pollfds[0].revents & libc::POLLIN != 0 {
                ipc_bus.borrow_mut().dispatch();
            }
        }

        tracing::info!(target: "typio.lifecycle", "shutting down");
        0
    }

    #[cfg(feature = "wayland")]
    /// Reload core/platform configuration and notify listeners.
    fn reload_config(&mut self) {
        let Some(instance) = self.instance.as_ref().cloned() else {
            return;
        };
        let reload = instance.borrow_mut().reload_config_rust();
        match reload {
            Ok(()) => {
                tracing::info!(target: "typio.lifecycle", "configuration reloaded");
                self.indicator_config = self.load_indicator_config();
                self.refresh_state_surfaces();
                // A reload may change the panel font (display.font_family /
                // display.font_size). Re-read and push it down so the
                // candidate text geometry reflects the new font, then force a
                // repaint. The family class is applied to the text context in
                // `panel.set_font_config` — see text_raster.rs.
                self.panel_font_config = self.load_display_font_config();
                if let Some(ref mut frontend) = self.frontend {
                    if let Some(panel) = frontend.panel_mut() {
                        panel.set_font_config(self.panel_font_config.clone());
                    }
                    let state = frontend.state_mut();
                    state.invalidate_panel_presentation();
                    if !state.composition.candidates.is_empty() {
                        state.mark_panel_dirty();
                    }
                }
            }
            _ => tracing::warn!(target: "typio.lifecycle", "configuration reload failed"),
        }
    }
    /// Re-sync the `StateController` with typio-runtime, then push
    /// the resulting state to every surface that mirrors it: the IPC
    /// bus (controller listeners), the tray icon + tooltip, and the
    /// tray menu snapshot.
    ///
    /// Called from two paths: the config-watcher reload callback (config
    /// may have changed the active engine/language), and the main-loop
    /// drain of `DaemonEvent::StateRefresh` (tray-driven engine/language
    /// switches that bypass the Rust controller).
    fn refresh_state_surfaces(&mut self) {
        let instance = match self.instance.as_ref() {
            Some(instance) => instance.clone(),
            None => return,
        };
        if let Some(ref mut controller) = self.state_controller {
            controller.sync();
            #[cfg(feature = "systray")]
            if let Some(ref tray) = self.tray {
                update_tray_from_controller(tray, controller, &instance);
            }
        }
        if let Some(ref ipc) = self.ipc_bus {
            ipc.borrow_mut()
                .emit(topics::RUNTIME_CHANGED, &serde_json::json!({}));
        }
        #[cfg(feature = "systray")]
        if let Some(ref tray) = self.tray {
            if let Some(snapshot) = build_tray_snapshot(&instance) {
                tray.set_menu_snapshot(snapshot);
            }
        }
    }

    fn run_without_uds(&mut self) -> i32 {
        tracing::info!(target: "typio.lifecycle", "running without UDS");

        #[cfg(feature = "wayland")]
        if self.frontend.is_some() && self.router.is_some() && self.repeat_timer.is_some() {
            return self.run_with_wayland(None);
        }

        let mut pollfd = libc::pollfd {
            fd: self.event_waker.fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        while !self.drain_events() {
            pollfd.revents = 0;
            let rc = unsafe { libc::poll(&mut pollfd, 1, -1) };
            if rc < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                tracing::error!(target: "typio.lifecycle", %error, "poll failed");
                return 1;
            }
        }
        0
    }

    /// Drain all pending daemon events and translate the signal-flag into
    /// the same model. Returns the resulting action set so the main loop
    /// can break / refresh / nothing in one place.
    ///
    /// - `Shutdown` and the SIGINT/SIGTERM flag both set `should_exit`.
    /// - `Restart` additionally records `saw_restart` for [`Self::finish`].
    /// - `StateRefresh` triggers a controller + tray + IPC re-sync via
    ///   [`Self::refresh_state_surfaces`].
    fn drain_events(&mut self) -> bool {
        if let Err(error) = self.event_waker.drain() {
            tracing::error!(target: "typio.lifecycle", %error, "failed to drain daemon eventfd");
            return true;
        }

        // Apply any SIGUSR1/SIGUSR2 log-level change on the loop thread (the
        // filter reload is not async-signal-safe, so the handler only flags).
        crate::diagnostics::apply_pending_level_signals();

        let mut should_exit = signals::take_shutdown_requested();
        let mut state_refresh = false;

        if let Some(rx) = self.event_rx.as_ref() {
            for event in rx.try_iter() {
                match event {
                    DaemonEvent::Shutdown => should_exit = true,
                    DaemonEvent::Restart => {
                        self.saw_restart = true;
                        should_exit = true;
                    }
                    DaemonEvent::StateRefresh => state_refresh = true,
                    #[cfg(feature = "systray")]
                    DaemonEvent::TrayAction(action) => {
                        if tray::apply_tray_action(self.instance.as_ref(), action, &self.event_tx) {
                            state_refresh = true;
                        }
                    }
                }
            }
        }

        if state_refresh {
            tracing::debug!(target: "typio.lifecycle", "StateRefresh received, refreshing state surfaces");
            self.refresh_state_surfaces();
            // StateRefresh covers every deliberate registry mutation:
            // the Ctrl+Shift language-switch chord, tray menu picks, and
            // IPC-driven switches (`typioctl language use …`). All are
            // user-initiated, so they go through the indicator's
            // no-gate deliberate-change path.
            #[cfg(feature = "wayland")]
            if self.frontend.is_some() {
                self.trigger_indicator_state_change();
            }
        }

        should_exit
    }

    /// Tear down runtime services.
    ///
    /// Drops runtime-dependent services before the shared instance. The
    /// router owns its input context and the voice controller may own an
    /// in-flight process handle, so explicit order keeps lifecycle effects
    /// deterministic even though all memory ownership is safe Rust.
    pub fn shutdown(&mut self) {
        // Drop the voice controller first: freeing its session joins any
        // in-flight inference thread and releases its process handle before
        // the registry shuts down.
        #[cfg(feature = "wayland")]
        drop(self.voice.take());
        drop(self.router.take());
        drop(self.repeat_timer.take());
        drop(self.frontend.take());
        drop(self.state_controller.take());
        if let Some(instance) = self.instance.take() {
            instance.borrow_mut().shutdown_rust();
        }
    }

    /// Finalize: exec on restart, then return the exit code.
    pub fn finish(self, exit_code: i32) -> i32 {
        if self.saw_restart && exit_code == 0 {
            tracing::info!(target: "typio.lifecycle", "restarting");
            let argv0 = self
                .argv
                .first()
                .cloned()
                .unwrap_or_else(|| CString::new("typio").unwrap());
            let mut ptrs: Vec<*const c_char> = self.argv.iter().map(|s| s.as_ptr()).collect();
            ptrs.push(std::ptr::null());
            unsafe {
                libc::execv(argv0.as_ptr(), ptrs.as_ptr());
            }
            tracing::error!(
                target: "typio.lifecycle",
                error = %std::io::Error::last_os_error(),
                "execv failed"
            );
            return 1;
        }
        exit_code
    }

    fn print_startup_banner(&self) {
        let version = env!("CARGO_PKG_VERSION");
        tracing::info!(target: "typio.startup", version, "starting typio");
    }
}

/// Arm or disarm the keyboard repeat timer based on the current modifier
/// state and the compositor's reported repeat preferences.
///
/// Used by the main loop after both the engine-consumed and
/// forwarded-key paths so both kinds of key repeat identically.
/// Auto-repeat is suppressed entirely when a repeat-suppressing
/// modifier (Ctrl / Alt / Super) is held, or when the compositor
/// advertises `rate == 0`. `mods_depressed` must already be in the
/// host-wide [`typio_host_types::Modifiers`] layout (see
/// `InputMethodState::effective_modifiers`), not the raw xkb wire mask.
#[cfg(feature = "wayland")]
fn arm_repeat(timer: &mut RepeatTimer, compositor_info: Option<(i32, i32)>, mods_depressed: u32) {
    if !typio_host_types::should_repeat_for_modifiers(typio_host_types::Modifiers(mods_depressed)) {
        let _ = timer.stop();
        return;
    }
    match repeat_timer::resolve_repeat_params(compositor_info) {
        Some((delay, interval)) => {
            let _ = timer.start(delay, interval);
        }
        None => {
            // Compositor reports rate == 0: do not repeat.
            let _ = timer.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::Mutex;

    /// Serialises tests that touch the shared signal flags so they do not
    /// race with each other when `cargo test` runs them in parallel.
    static SIGNAL_FLAG_LOCK: Mutex<()> = Mutex::new(());

    fn poll_readable(fd: libc::c_int, timeout_ms: i32) -> bool {
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        assert!(ready >= 0, "poll failed: {}", io::Error::last_os_error());
        ready == 1 && pollfd.revents & libc::POLLIN != 0
    }

    #[test]
    fn daemon_events_drive_drain_results() {
        let _guard = SIGNAL_FLAG_LOCK.lock().unwrap();

        // Reset the signal flag so prior tests don't leak.
        signals::reset_shutdown_flag();

        // Build a minimal App with just the event channel wired. Other
        // fields are empty; drain_events does not touch them unless an
        // event triggers StateRefresh (which we don't send here).
        let (tx, rx, event_waker) = event_channel::channel().unwrap();
        let mut app = App {
            argv: vec![],
            options: AppOptions {
                config_dir: None,
                data_dir: None,
                engine_dirs: vec![],
                socket_path: None,
                verbosity: 0,
            },
            instance: None,
            state_controller: None,
            ipc_bus: None,
            #[cfg(feature = "systray")]
            tray: None,
            #[cfg(feature = "wayland")]
            frontend: None,
            #[cfg(feature = "wayland")]
            router: None,
            #[cfg(feature = "wayland")]
            repeat_timer: None,
            #[cfg(feature = "wayland")]
            resume_signal: None,
            #[cfg(feature = "wayland")]
            focus_driver: None,
            #[cfg(feature = "wayland")]
            voice: None,
            indicator: None,
            indicator_config: IndicatorConfig::default(),
            panel_font_config: font_config::PanelFontConfig::default(),
            #[cfg(feature = "wayland")]
            indicator_timer: None,
            #[cfg(feature = "wayland")]
            voice_status_timer: None,
            #[cfg(feature = "wayland")]
            voice_last_state: typio_runtime::voice::types::VoiceState::Idle,
            #[cfg(feature = "wayland")]
            voice_pending_banner: None,
            config_watcher: None,
            #[cfg(feature = "wayland")]
            last_idle_reap: None,
            event_tx: tx,
            event_rx: Some(rx),
            event_waker,
            saw_restart: false,
        };

        // Empty channel + clear signal flag → no exit.
        assert!(!app.drain_events());
        assert!(!app.saw_restart);

        // Shutdown via channel.
        let _ = app.event_tx.send(DaemonEvent::Shutdown);
        assert!(poll_readable(app.event_waker.fd(), 0));
        assert!(app.drain_events());
        assert!(!poll_readable(app.event_waker.fd(), 0));
        assert!(!app.saw_restart);

        // Restart sets both saw_restart and should_exit.
        let _ = app.event_tx.send(DaemonEvent::Restart);
        assert!(app.drain_events());
        assert!(app.saw_restart);

        // A handler running on a background thread wakes the main-thread
        // poll, reproducing the delivery pattern that previously left the
        // daemon stuck indefinitely after SIGTERM.
        app.saw_restart = false;
        signals::set_reactor_wake_fd_for_test(app.event_waker.fd());
        std::thread::spawn(|| signals::invoke_signal_handler_for_test(libc::SIGTERM))
            .join()
            .unwrap();
        assert!(poll_readable(app.event_waker.fd(), 1_000));
        assert!(app.drain_events());
        assert!(!poll_readable(app.event_waker.fd(), 0));
        assert!(!app.saw_restart); // signal path is Shutdown-only

        signals::set_reactor_wake_fd_for_test(-1);
        signals::reset_shutdown_flag();
    }
}
