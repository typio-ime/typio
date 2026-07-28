//! Signal handlers and process-global callback trampolines.
//!
//! Split out of `mod.rs` so the daemon lifecycle file owns *what happens
//! on a signal* (drain, refresh, exit) rather than *how the kernel
//! delivers it*. The trampolines here are intentionally tiny: they do
//! the minimum async-signal-safe work (set a flag and write an eventfd) and
//! let the main loop react on its own thread.

use std::ffi::c_void;
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use super::{DaemonEvent, DaemonEventSender, ReactorWaker};

/// Async-signal-safe shutdown flag.
///
/// Only the SIGINT/SIGTERM handler writes this. The main loop translates
/// it into a daemon exit on the next reactor step. Non-signal paths
/// (`DaemonEvent::Shutdown` via the event channel) must NOT touch this
/// flag — keeping it signal-only preserves async-signal-safety.
pub(super) static SHUTDOWN_FROM_SIGNAL: AtomicBool = AtomicBool::new(false);

/// Process-global sender for the mode-changed callback. Stored in a
/// `OnceLock` because the C ABI callback holds a raw `user_data` pointer
/// that must be valid for the instance's lifetime, and there is only one
/// daemon per process. The `Mutex` makes `&Sender` safely shareable
/// across the engine communication thread (where out-of-process engine
/// responses fire the callback) and the main loop thread.
static MODE_CALLBACK_TX: OnceLock<std::sync::Mutex<DaemonEventSender>> = OnceLock::new();

/// Raw eventfd used only by the async signal handler.
///
/// `App` owns the descriptor through [`ReactorWaker`]. The daemon is a
/// process singleton, so the descriptor remains valid from handler
/// installation until `execv` or process exit.
static REACTOR_WAKE_FD: AtomicI32 = AtomicI32::new(-1);

/// Install the wakeable sender used by [`mode_changed_trampoline`]. Called
/// once from [`crate::app::App::init`] after the daemon event channel is
/// wired. Subsequent calls are no-ops (the first sender wins), matching the
/// singleton nature of the trampoline.
pub(super) fn set_mode_callback_tx(tx: DaemonEventSender) {
    let _ = MODE_CALLBACK_TX.set(std::sync::Mutex::new(tx));
}

/// Swap the shutdown flag and return the previous value. Used by the
/// main loop's per-step drain to translate a signal into the same exit
/// path as `DaemonEvent::Shutdown`.
pub(super) fn take_shutdown_requested() -> bool {
    SHUTDOWN_FROM_SIGNAL.swap(false, Ordering::AcqRel)
}

/// Reset the shutdown flag. Used by tests that touch the signal path.
#[cfg(test)]
pub(super) fn reset_shutdown_flag() {
    SHUTDOWN_FROM_SIGNAL.store(false, Ordering::SeqCst);
}

extern "C" fn signal_handler(sig: libc::c_int) {
    match sig {
        libc::SIGINT | libc::SIGTERM => {
            SHUTDOWN_FROM_SIGNAL.store(true, Ordering::SeqCst);
        }
        libc::SIGUSR1 => crate::diagnostics::request_raise_level(),
        libc::SIGUSR2 => crate::diagnostics::request_reset_level(),
        _ => return,
    }
    wake_reactor_from_signal();
}

/// Wake the main reactor using only async-signal-safe operations.
fn wake_reactor_from_signal() {
    let fd = REACTOR_WAKE_FD.load(Ordering::Acquire);
    if fd < 0 {
        return;
    }
    let value = 1u64.to_ne_bytes();
    unsafe {
        // `write(2)` is async-signal-safe. A nonblocking eventfd either
        // accepts the full eight-byte counter or returns EAGAIN because it
        // is already readable; both outcomes satisfy the wakeup contract.
        let _ = libc::write(fd, value.as_ptr().cast::<libc::c_void>(), value.len());
    }
}

/// C trampoline for `TypioKeyboardModeChangedCallback`. Fires when an
/// engine reports a **deliberate** mode change (e.g. rime switching schema
/// or toggling 中/A). Marshals to the main loop via `DaemonEvent::StateRefresh`;
/// the main loop then reads the fresh mode from
/// `typio_instance_get_last_keyboard_mode` and triggers the indicator's
/// no-gate deliberate-change path.
///
/// The first parameter uses the **opaque** `typio_abi::TypioInstance`
/// (not `typio::instance::TypioInstance`) to match the callback typedef
/// exactly. The actual pointer is to the real struct; we never dereference
/// it here, so the opacity is harmless.
pub(super) extern "C" fn mode_changed_trampoline(
    _instance: *mut typio_abi::TypioInstance,
    _mode: *const typio_abi::TypioKeyboardEngineMode,
    _user_data: *mut c_void,
) {
    if let Some(mutex) = MODE_CALLBACK_TX.get() {
        if let Ok(tx) = mutex.lock() {
            let _ = tx.send(DaemonEvent::StateRefresh);
        }
    }
}

pub(super) fn install_signal_handlers(waker: &ReactorWaker) -> io::Result<()> {
    REACTOR_WAKE_FD.store(waker.fd(), Ordering::Release);
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGUSR1, libc::SIGUSR2] {
        install_signal_handler(signal)?;
    }
    Ok(())
}

fn install_signal_handler(signal: libc::c_int) -> io::Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = signal_handler as *const () as libc::sighandler_t;
    // Preserve unrelated blocking I/O on whichever process thread receives
    // the signal. The eventfd independently wakes the main reactor, so it
    // does not rely on interrupting that thread's `poll(2)` call.
    action.sa_flags = libc::SA_RESTART;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(signal, &action, std::ptr::null_mut()) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn set_reactor_wake_fd_for_test(fd: libc::c_int) {
    REACTOR_WAKE_FD.store(fd, Ordering::SeqCst);
}

#[cfg(test)]
pub(super) fn invoke_signal_handler_for_test(signal: libc::c_int) {
    signal_handler(signal);
}
