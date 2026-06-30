//! Logging — IoC-first structured logging backend.
//!
//! The library never decides where logs go.  It only produces log records and
//! forwards them to a host-provided callback.  If no callback is installed,
//! records are silently dropped (but retained in a ring buffer for crash dumps).
//!
//! Rust code inside libtypio uses the standard `log` crate macros
//! (`log::info!`, `log::warn!`, etc.).  C engines/plugins call `typio_log_emit()`
//! or the `typio_log` / `typio_logf` convenience wrappers in `typio/abi/log.h`.

use crate::types::{TypioLogCallback, TypioLogEvent, TypioLogLevel};
use log::{Level, Log, Metadata, Record};
use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CAPACITY: usize = 256;

/* -------------------------------------------------------------------------- */
/* C ABI types                                                                */
/* -------------------------------------------------------------------------- */

/* -------------------------------------------------------------------------- */
/* Internal ring-buffer entry                                                 */
/* -------------------------------------------------------------------------- */

struct LogEntry {
    timestamp: SystemTime,
    level: TypioLogLevel,
    message: String,
    domain: String,
    file: String,
    line: u32,
}

/* -------------------------------------------------------------------------- */
/* Logger implementation                                                      */
/* -------------------------------------------------------------------------- */

/// Internal logger that forwards to a host callback and retains recent entries in a ring buffer.
pub struct TypioLogger {
    level: AtomicU8,
    callback: Mutex<Option<(TypioLogCallback, *mut c_void)>>,
    recent: Mutex<VecDeque<LogEntry>>,
    capacity: AtomicUsize,
}

// SAFETY: The `*mut c_void` is an opaque user-data pointer that is only ever
// passed through to the host callback.  It is never dereferenced by Rust code.
unsafe impl Send for TypioLogger {}
unsafe impl Sync for TypioLogger {}

impl TypioLogger {
    fn new() -> Self {
        Self {
            level: AtomicU8::new(TypioLogLevel::TypioLogInfo as u8),
            callback: Mutex::new(None),
            recent: Mutex::new(VecDeque::with_capacity(DEFAULT_CAPACITY)),
            capacity: AtomicUsize::new(DEFAULT_CAPACITY),
        }
    }

    fn level(&self) -> TypioLogLevel {
        match self.level.load(Ordering::Relaxed) {
            0 => TypioLogLevel::TypioLogTrace,
            1 => TypioLogLevel::TypioLogDebug,
            2 => TypioLogLevel::TypioLogInfo,
            3 => TypioLogLevel::TypioLogWarning,
            _ => TypioLogLevel::TypioLogError,
        }
    }

    fn set_level(&self, level: TypioLogLevel) {
        self.level.store(level as u8, Ordering::Relaxed);
    }

    fn set_callback(&self, cb: Option<TypioLogCallback>, user_data: *mut c_void) {
        let mut guard = self.callback.lock().unwrap();
        *guard = cb.map(|c| (c, user_data));
    }

    fn set_capacity(&self, capacity: usize) {
        self.capacity.store(capacity, Ordering::Relaxed);
        let mut recent = self.recent.lock().unwrap();
        while recent.len() > capacity {
            recent.pop_front();
        }
    }

    fn dump_recent(&self, path: &Path) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::Write;

        let recent = self.recent.lock().unwrap();
        let mut file = File::create(path)?;
        for entry in recent.iter() {
            let ts = entry
                .timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let level_str = match entry.level {
                TypioLogLevel::TypioLogTrace => "TRACE",
                TypioLogLevel::TypioLogDebug => "DEBUG",
                TypioLogLevel::TypioLogInfo => "INFO",
                TypioLogLevel::TypioLogWarning => "WARN",
                TypioLogLevel::TypioLogError => "ERROR",
            };
            writeln!(
                file,
                "[{}] [{}] [{}] {}:{} {}",
                ts, level_str, entry.domain, entry.file, entry.line, entry.message
            )?;
        }
        Ok(())
    }
}

impl Log for TypioLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let level = self.level();
        let required = match metadata.level() {
            Level::Error => TypioLogLevel::TypioLogError,
            Level::Warn => TypioLogLevel::TypioLogWarning,
            Level::Info => TypioLogLevel::TypioLogInfo,
            Level::Debug => TypioLogLevel::TypioLogDebug,
            Level::Trace => TypioLogLevel::TypioLogTrace,
        };
        (required as u8) >= (level as u8)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let level = match record.level() {
            Level::Error => TypioLogLevel::TypioLogError,
            Level::Warn => TypioLogLevel::TypioLogWarning,
            Level::Info => TypioLogLevel::TypioLogInfo,
            Level::Debug => TypioLogLevel::TypioLogDebug,
            Level::Trace => TypioLogLevel::TypioLogTrace,
        };

        let message = format!("{}", record.args());
        let domain = record.module_path().unwrap_or("typio").to_string();
        let file = record.file().unwrap_or("unknown").to_string();
        let line = record.line().unwrap_or(0);
        let timestamp = SystemTime::now();

        // Store in ring buffer.
        {
            let mut recent = self.recent.lock().unwrap();
            let cap = self.capacity.load(Ordering::Relaxed);
            if recent.len() >= cap {
                recent.pop_front();
            }
            recent.push_back(LogEntry {
                timestamp,
                level,
                message: message.clone(),
                domain: domain.clone(),
                file: file.clone(),
                line,
            });
        }

        // Forward to host callback if one is installed.
        if let Some((cb, user_data)) = *self.callback.lock().unwrap() {
            let c_message = CString::new(message.as_bytes())
                .unwrap_or_else(|_| CString::new("<invalid>").unwrap());
            let c_domain = CString::new(domain.as_bytes())
                .unwrap_or_else(|_| CString::new("unknown").unwrap());
            let c_file =
                CString::new(file.as_bytes()).unwrap_or_else(|_| CString::new("unknown").unwrap());
            let timestamp_ms = timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let event = TypioLogEvent {
                level,
                message: c_message.as_ptr(),
                domain: c_domain.as_ptr(),
                file: c_file.as_ptr(),
                line,
                timestamp_ms,
            };

            cb(&event, user_data);
        }
    }

    fn flush(&self) {}
}

/* -------------------------------------------------------------------------- */
/* Global proxy                                                               */
/* -------------------------------------------------------------------------- */

/// The canonical logger instance.  Initialised on first `typio_logger_init()`.
static LOGGER: OnceLock<TypioLogger> = OnceLock::new();

/// Proxy type registered with the `log` crate.  It forwards to `LOGGER`.
struct GlobalLogger;

impl Log for GlobalLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        LOGGER.get().map_or(false, |l| l.enabled(metadata))
    }

    fn log(&self, record: &Record) {
        if let Some(l) = LOGGER.get() {
            l.log(record);
        }
    }

    fn flush(&self) {
        if let Some(l) = LOGGER.get() {
            l.flush();
        }
    }
}

static GLOBAL_LOGGER: GlobalLogger = GlobalLogger;
static INIT_LOGGER: Once = Once::new();

/* -------------------------------------------------------------------------- */
/* C ABI                                                                      */
/* -------------------------------------------------------------------------- */

/// Initialise the logging subsystem.
///
/// Idempotent: safe to call multiple times.  Must be called before any
/// `log::info!` / `typio_log_emit` usage if the host wants to capture logs.
#[no_mangle]
pub extern "C" fn typio_logger_init() -> bool {
    LOGGER.get_or_init(TypioLogger::new);

    let mut ok = true;
    INIT_LOGGER.call_once(|| {
        if log::set_logger(&GLOBAL_LOGGER).is_err() {
            ok = false;
        } else {
            log::set_max_level(log::LevelFilter::Trace);
        }
    });
    ok
}

/// Set (or clear) the host-provided log callback.
///
/// `callback` may be `NULL` to disable host-side output.  When no callback is
/// set, log records are only retained in the internal ring buffer.
#[no_mangle]
pub extern "C" fn typio_logger_set_callback(
    callback: Option<TypioLogCallback>,
    user_data: *mut c_void,
) {
    if let Some(logger) = LOGGER.get() {
        logger.set_callback(callback, user_data);
    }
}

/// Set the minimum log level.  Default is `TYPIO_LOG_INFO`.
#[no_mangle]
pub extern "C" fn typio_logger_set_level(level: TypioLogLevel) {
    if let Some(logger) = LOGGER.get() {
        logger.set_level(level);
    }
}

/// Get the current minimum log level.
#[no_mangle]
pub extern "C" fn typio_logger_get_level() -> TypioLogLevel {
    LOGGER
        .get()
        .map_or(TypioLogLevel::TypioLogInfo, |l| l.level())
}

/// Set the capacity of the recent-log ring buffer.  Default is 256.
#[no_mangle]
pub extern "C" fn typio_logger_set_recent_capacity(capacity: usize) {
    if let Some(logger) = LOGGER.get() {
        logger.set_capacity(capacity);
    }
}

/// Dump the recent-log ring buffer to a file.  Returns `true` on success.
#[no_mangle]
pub extern "C" fn typio_logger_dump_recent(path: *const c_char) -> bool {
    if path.is_null() {
        return false;
    }
    let path_str = unsafe { CStr::from_ptr(path).to_string_lossy() };
    let path_ref = Path::new(path_str.as_ref());

    if let Some(parent) = path_ref.parent() {
        if !parent.exists() && std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }

    LOGGER
        .get()
        .and_then(|l| l.dump_recent(path_ref).ok())
        .is_some()
}

/// Shut down the logging subsystem.
///
/// Clears the callback, resets the level to `TYPIO_LOG_INFO`, and empties the
/// ring buffer.  The global logger remains registered with the `log` crate but
/// becomes a no-op.
#[no_mangle]
pub extern "C" fn typio_logger_shutdown() {
    if let Some(logger) = LOGGER.get() {
        logger.set_callback(None, std::ptr::null_mut());
        logger.set_level(TypioLogLevel::TypioLogInfo);
        logger.recent.lock().unwrap().clear();
    }
}

/// Log a pre-formatted message from C code.
///
/// C engines should use the `typio_log` / `typio_logf` inline helpers in
/// `typio/abi/log.h` rather than calling this directly.
#[no_mangle]
pub extern "C" fn typio_log_emit(level: TypioLogLevel, message: *const c_char) {
    if message.is_null() {
        return;
    }
    let msg = unsafe { CStr::from_ptr(message).to_string_lossy() };
    match level {
        TypioLogLevel::TypioLogTrace => log::trace!(target: "typio::c_api", "{}", msg),
        TypioLogLevel::TypioLogDebug => log::debug!(target: "typio::c_api", "{}", msg),
        TypioLogLevel::TypioLogInfo => log::info!(target: "typio::c_api", "{}", msg),
        TypioLogLevel::TypioLogWarning => log::warn!(target: "typio::c_api", "{}", msg),
        TypioLogLevel::TypioLogError => log::error!(target: "typio::c_api", "{}", msg),
    }
}

/// Internal helper for Rust code to log via the C ABI path.
pub(crate) fn log_msg(level: TypioLogLevel, msg: &str) {
    if let Ok(cmsg) = CString::new(msg) {
        let _ = typio_logger_init();
        typio_log_emit(level, cmsg.as_ptr());
    }
}
