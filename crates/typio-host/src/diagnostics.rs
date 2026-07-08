//! Runtime diagnostics and structured logging setup.
//!
//! # Logging conventions
//!
//! Every runtime diagnostic in the daemon flows through `tracing` — there is
//! no parallel `eprintln!`/`println!` channel for runtime output. The only
//! exception is the pre-logging fatal CLI error in `bin/typio.rs`. Two axes
//! carry the meaning:
//!
//! ## Levels (what)
//!
//! - `error!` — a failure that aborts the current operation or the daemon
//!   (Wayland I/O loss, `poll` failure, fatal startup error).
//! - `warn!`  — a degraded-but-continuing condition the operator should see
//!   (an engine failed to load, the tray did not register, UDS bind failed).
//! - `info!`  — notable lifecycle milestones: startup self-check (registered
//!   engines, connected surfaces), "running", config reload, shutdown.
//! - `debug!` — per-event diagnostics: indicator/panel/voice driving, tray
//!   actions, switch chords. The high-volume-but-readable stream.
//! - `trace!` — per-keystroke routing and per-frame timing.
//!
//! ## Targets (where), as `typio.<subsystem>[.<area>]`
//!
//! `typio.startup`, `typio.lifecycle`, `typio.indicator`, `typio.voice`,
//! `typio.tray`, `typio.config`, `typio.panel.*`, `typio.wayland.*`,
//! `typio.engine.*`, `typio.input.*`. Targets let
//! `RUST_LOG` refine one subsystem (e.g. `RUST_LOG=typio.indicator=debug`)
//! without raising the global floor.
//!
//! ## Runtime level adjustment
//!
//! The global floor is reloadable while the daemon runs: `SIGUSR1` raises it
//! one step (`info`→`debug`→`trace`), `SIGUSR2` resets it to the startup
//! level. The signal handlers only set an atomic flag (async-signal-safe);
//! the actual filter swap — which is *not* signal-safe — runs on the main
//! loop via [`apply_pending_level_signals`].

use std::io::IsTerminal;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::reload;
use tracing_subscriber::{EnvFilter, Registry, fmt, prelude::*};

/// Handle to the reloadable global filter, installed once by
/// [`init_logging`]. The `Registry` type parameter is the subscriber the
/// reload layer sits directly on top of (`registry().with(reload_layer)`).
static RELOAD_HANDLE: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();

/// Global log floor as a step: `0`=info, `1`=debug, `2`=trace. Mirrors CLI
/// verbosity and is the value `SIGUSR1`/`SIGUSR2` move around.
static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(0);

/// The floor selected at startup, restored by `SIGUSR2`.
static STARTUP_LEVEL: AtomicU8 = AtomicU8::new(0);

/// Set by the `SIGUSR1` handler; drained on the main loop.
static RAISE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Set by the `SIGUSR2` handler; drained on the main loop.
static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Map a level step to the `EnvFilter` directive that sets it as the floor.
fn level_directive(level: u8) -> &'static str {
    match level {
        0 => "info",
        1 => "debug",
        _ => "trace",
    }
}

/// Build the global filter for `level`: `RUST_LOG` (per-target directives)
/// underneath a bare-level floor. Re-reads `RUST_LOG` on every rebuild so
/// per-target overrides survive a runtime level change.
fn build_filter(level: u8) -> EnvFilter {
    EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy()
        .add_directive(
            level_directive(level)
                .parse()
                .unwrap_or_else(|_| LevelFilter::INFO.into()),
        )
}

/// Initialize structured logging once for the daemon process.
///
/// `RUST_LOG` is the primary filter surface and refines individual targets.
/// The CLI verbosity sets the global floor underneath it: bare `typio` logs
/// at `info` (startup report + warnings + errors), `-v` adds `debug`
/// (per-event subsystem diagnostics), `-vv` adds `trace` (key routing,
/// frame timing). This mapping matches `docs/reference/cli.md`. The floor is
/// reloadable at runtime — see [`apply_pending_level_signals`].
pub fn init_logging(verbosity: u8) {
    static INIT: OnceLock<()> = OnceLock::new();
    let _ = INIT.get_or_init(|| {
        let level = verbosity.min(2);
        CURRENT_LEVEL.store(level, Ordering::SeqCst);
        STARTUP_LEVEL.store(level, Ordering::SeqCst);

        let (filter, handle) = reload::Layer::new(build_filter(level));
        let _ = RELOAD_HANDLE.set(handle);

        // Colorize only for an interactive terminal. Under systemd the
        // daemon's stderr is the journal (or a redirected file), where ANSI
        // escapes are noise — keep that output plain.
        let ansi = std::io::stderr().is_terminal();
        let layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(ansi)
            .with_target(true)
            .with_thread_ids(false)
            .with_thread_names(false)
            .compact();

        tracing_subscriber::registry()
            .with(filter)
            .with(layer)
            .init();
    });
}

/// Request a one-step level raise (`SIGUSR1`). Async-signal-safe: only an
/// atomic store, applied later by [`apply_pending_level_signals`].
pub(crate) fn request_raise_level() {
    RAISE_REQUESTED.store(true, Ordering::SeqCst);
}

/// Request a reset to the startup level (`SIGUSR2`). Async-signal-safe.
pub(crate) fn request_reset_level() {
    RESET_REQUESTED.store(true, Ordering::SeqCst);
}

/// Swap the global filter to `level` and record it. No-op if logging was
/// never initialized.
fn apply_level(level: u8) {
    let level = level.min(2);
    let Some(handle) = RELOAD_HANDLE.get() else {
        return;
    };
    if handle.reload(build_filter(level)).is_ok() {
        CURRENT_LEVEL.store(level, Ordering::SeqCst);
        // info is included at every floor, so this confirmation is always
        // visible after the reload.
        tracing::info!(
            target: "typio.lifecycle",
            level = level_directive(level),
            "log level changed"
        );
    }
}

/// Apply any level change requested by `SIGUSR1`/`SIGUSR2`. Called once per
/// main-loop tick; the filter reload is not async-signal-safe, so it must
/// run here rather than in the signal handler. Cheap when nothing is pending
/// (two relaxed-ish atomic swaps).
pub fn apply_pending_level_signals() {
    if RESET_REQUESTED.swap(false, Ordering::SeqCst) {
        apply_level(STARTUP_LEVEL.load(Ordering::SeqCst));
    }
    if RAISE_REQUESTED.swap(false, Ordering::SeqCst) {
        let next = CURRENT_LEVEL.load(Ordering::SeqCst).saturating_add(1);
        apply_level(next);
    }
}
