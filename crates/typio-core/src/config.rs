//! Configuration management — Rust implementation of `typio/config.h`

mod getters;
mod parse;
mod serialize;
mod setters;

pub use getters::*;
pub use setters::*;

use crate::types::*;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_char};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Maximum wall-clock time a blocking config-file read may occupy. A config
/// file is a few KB; on a healthy local filesystem this completes in
/// microseconds. The bound exists for the pathological case: a config file on
/// a stalled NFS hard-mount or wedged FUSE filesystem, where a bare
/// `read_to_string` would block the (single-threaded) host loop indefinitely.
/// On timeout the load fails and the previous config is retained.
const CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONFIG_READS_IN_FLIGHT: usize = 4;
static CONFIG_READS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/* -------------------------------------------------------------------------- */
/* Internal representation                                                    */
/* -------------------------------------------------------------------------- */

/// A single config value.
#[derive(Clone, Debug)]
pub enum ConfigValue {
    /// String value.
    String(CString),
    /// Signed 32-bit integer.
    Int(i32),
    /// Boolean.
    Bool(bool),
    /// 64-bit floating point.
    Float(f64),
    /// Ordered array of values.
    Array(Vec<ConfigValue>),
    /// Nested config object.
    Object(Box<Config>),
}

/// Opaque configuration object.
pub struct Config {
    pub(crate) entries: HashMap<String, ConfigValue>,
    pub(crate) user_keys: HashSet<String>,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            entries: self.entries.clone(),
            user_keys: self.user_keys.clone(),
        }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("entries", &self.entries)
            .field("user_keys", &self.user_keys)
            .finish()
    }
}

impl Config {
    pub(crate) fn new() -> Self {
        Config {
            entries: HashMap::new(),
            user_keys: HashSet::new(),
        }
    }

    pub(crate) fn set_value(&mut self, key: String, value: ConfigValue) {
        self.user_keys.insert(key.clone());
        self.entries.insert(key, value);
    }

    pub(crate) fn set_default_value(&mut self, key: String, value: ConfigValue) {
        self.entries.insert(key, value);
    }

    /// Borrow the value stored for a dotted configuration key.
    pub fn value(&self, key: &str) -> Option<&ConfigValue> {
        self.entries.get(key)
    }

    /// Iterate over stored dotted keys and their values.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ConfigValue)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    /// Whether a key came from user input rather than schema defaults.
    pub fn is_user_value(&self, key: &str) -> bool {
        self.user_keys.contains(key)
    }

    fn from_toml(value: &toml::Value, prefix: &str) -> Self {
        let mut config = Config::new();
        config.populate_from_toml(value, prefix);
        config
    }

    fn populate_from_toml(&mut self, value: &toml::Value, prefix: &str) {
        match value {
            toml::Value::Table(table) => {
                for (k, v) in table.iter() {
                    let full_key = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{}.{}", prefix, k)
                    };
                    match v {
                        toml::Value::Table(_) => {
                            self.populate_from_toml(v, &full_key);
                        }
                        _ => {
                            if let Some(cv) = parse::toml_to_config_value(v) {
                                self.set_value(full_key, cv);
                            }
                        }
                    }
                }
            }
            _ => {
                if let Some(cv) = parse::toml_to_config_value(value) {
                    let key = if prefix.is_empty() {
                        "value".to_string()
                    } else {
                        prefix.to_string()
                    };
                    self.set_value(key, cv);
                }
            }
        }
    }
}

/* -------------------------------------------------------------------------- */
/* C FFI — Constructors & lifecycle                                           */
/* -------------------------------------------------------------------------- */

/// Create a new empty config object.
///
/// Returns a pointer that must be freed with `typio_config_free`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_new() -> *mut Config {
    Box::into_raw(Box::new(Config::new()))
}

/// Load a config from a TOML file.
///
/// Returns a pointer that must be freed with `typio_config_free`, or NULL on error.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_load_file(path: *const c_char) -> *mut Config {
    if path.is_null() {
        return ptr::null_mut();
    }
    let path_str = unsafe { CStr::from_ptr(path).to_string_lossy() };
    // Read off the main loop with a bounded deadline. `fs::read_to_string` is a
    // raw blocking syscall with no timeout; on a stalled network/edge
    // filesystem it can hang the host for the kernel's full RPC timeout
    // (seconds to unbounded). A short-lived reader thread + channel timeout
    // bounds it; the global cap prevents repeated timeouts from accumulating
    // unbounded stuck threads. On failure the previous config survives.
    let path_buf = Path::new(&*path_str).to_path_buf();
    if CONFIG_READS_IN_FLIGHT
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_CONFIG_READS_IN_FLIGHT).then_some(count + 1)
        })
        .is_err()
    {
        return ptr::null_mut();
    }
    let (tx, rx) = mpsc::channel();
    if thread::Builder::new()
        .name("typio-config-read".into())
        .spawn(move || {
            let result = fs::read_to_string(&path_buf);
            CONFIG_READS_IN_FLIGHT.fetch_sub(1, Ordering::Release);
            let _ = tx.send(result);
        })
        .is_err()
    {
        CONFIG_READS_IN_FLIGHT.fetch_sub(1, Ordering::Release);
        return ptr::null_mut();
    }
    let content = match rx.recv_timeout(CONFIG_READ_TIMEOUT) {
        Ok(Ok(c)) => c,
        Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Timeout) => return ptr::null_mut(),
        Err(mpsc::RecvTimeoutError::Disconnected) => return ptr::null_mut(),
    };
    // `load_string` only borrows the pointer, so keep ownership locally and let
    // the CString drop at end of scope. Using `into_raw()` here leaked the
    // buffer on every config load.
    let content_c = match CString::new(content) {
        Ok(c) => c,
        Err(_) => return ptr::null_mut(),
    };
    typio_config_load_string(content_c.as_ptr())
}

/// Load a config from a TOML or INI-like string.
///
/// Returns a pointer that must be freed with `typio_config_free`, or NULL on error.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_load_string(content: *const c_char) -> *mut Config {
    if content.is_null() {
        return ptr::null_mut();
    }
    let s = unsafe { CStr::from_ptr(content).to_string_lossy() };
    let parsed: toml::Value = match s.parse() {
        Ok(v) => v,
        Err(_) => {
            return parse::parse_ini_like(&s);
        }
    };
    Box::into_raw(Box::new(Config::from_toml(&parsed, "")))
}

/// Free a config object and all owned values.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_free(config: *mut Config) {
    if !config.is_null() {
        unsafe { drop(Box::from_raw(config)) };
    }
}

/// Save a config object to a file atomically (write-then-rename).
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_save_file(
    config: *const Config,
    path: *const c_char,
) -> TypioResult {
    if config.is_null() || path.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &*config };
    let path_str = unsafe { CStr::from_ptr(path).to_string_lossy() };
    let path_obj = Path::new(&*path_str);

    let content = serialize::config_to_string_internal(cfg);

    let tmp_path = path_obj.with_extension("tmp");
    match fs::File::create(&tmp_path) {
        Ok(mut file) => {
            if file.write_all(content.as_bytes()).is_err() {
                let _ = fs::remove_file(&tmp_path);
                return TypioResult::TypioError;
            }
            if file.sync_all().is_err() {
                let _ = fs::remove_file(&tmp_path);
                return TypioResult::TypioError;
            }
            drop(file);
            if fs::rename(&tmp_path, path_obj).is_err() {
                let _ = fs::remove_file(&tmp_path);
                return TypioResult::TypioError;
            }
            TypioResult::TypioOk
        }
        Err(_) => TypioResult::TypioError,
    }
}

/// Serialize a config object to a TOML string.
///
/// Caller must free the returned string with `typio_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_to_string(config: *const Config) -> *mut c_char {
    if config.is_null() {
        return ptr::null_mut();
    }
    let cfg = unsafe { &*config };
    let content = serialize::config_to_string_internal(cfg);
    match CString::new(content) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};
    use std::ptr;

    #[test]
    fn config_new_and_free() {
        let cfg = typio_config_new();
        assert!(!cfg.is_null());
        typio_config_free(cfg);
    }

    #[test]
    fn config_merge_preserves_user_and_default_sources() {
        let mut source = Config::new();
        source.set_default_value("default.only".into(), ConfigValue::Bool(true));
        source.set_value("user.only".into(), ConfigValue::Int(7));
        let mut destination = Config::new();

        assert_eq!(
            typio_config_merge(&mut destination, &source),
            TypioResult::TypioOk
        );
        assert!(!destination.is_user_value("default.only"));
        assert!(destination.is_user_value("user.only"));
        let serialized = serialize::config_to_string_internal(&destination);
        assert!(!serialized.contains("[default]"));
        assert!(serialized.contains("only = 7"));
    }

    #[test]
    fn config_string_roundtrip() {
        let cfg = typio_config_new();
        let key = CString::new("test.string").unwrap();
        let val = CString::new("hello").unwrap();
        let ret = typio_config_set_string(cfg, key.as_ptr(), val.as_ptr());
        assert_eq!(ret, TypioResult::TypioOk);

        let got = typio_config_get_string(cfg, key.as_ptr(), ptr::null());
        assert!(!got.is_null());
        let s = unsafe { CStr::from_ptr(got) }.to_str().unwrap();
        assert_eq!(s, "hello");

        typio_config_free(cfg);
    }

    #[test]
    fn config_int_roundtrip() {
        let cfg = typio_config_new();
        let key = CString::new("test.int").unwrap();
        let ret = typio_config_set_int(cfg, key.as_ptr(), 42);
        assert_eq!(ret, TypioResult::TypioOk);

        let got = typio_config_get_int(cfg, key.as_ptr(), 0);
        assert_eq!(got, 42);

        typio_config_free(cfg);
    }

    #[test]
    fn config_bool_roundtrip() {
        let cfg = typio_config_new();
        let key = CString::new("test.bool").unwrap();
        let ret = typio_config_set_bool(cfg, key.as_ptr(), true);
        assert_eq!(ret, TypioResult::TypioOk);

        let got = typio_config_get_bool(cfg, key.as_ptr(), false);
        assert!(got);

        typio_config_free(cfg);
    }

    #[test]
    fn config_float_roundtrip() {
        let cfg = typio_config_new();
        let key = CString::new("test.float").unwrap();
        let ret = typio_config_set_float(cfg, key.as_ptr(), 3.125);
        assert_eq!(ret, TypioResult::TypioOk);

        let got = typio_config_get_float(cfg, key.as_ptr(), 0.0);
        assert!((got - 3.125).abs() < 0.001);

        typio_config_free(cfg);
    }

    #[test]
    fn config_save_and_load() {
        let cfg = typio_config_new();
        let key = CString::new("test.value").unwrap();
        let val = CString::new("persisted").unwrap();
        typio_config_set_string(cfg, key.as_ptr(), val.as_ptr());

        let path = std::env::temp_dir().join("libtypio_test_config.toml");
        let path_c = CString::new(path.to_str().unwrap()).unwrap();
        let ret = typio_config_save_file(cfg, path_c.as_ptr());
        assert_eq!(ret, TypioResult::TypioOk);
        typio_config_free(cfg);

        let loaded = typio_config_load_file(path_c.as_ptr());
        assert!(!loaded.is_null());
        let got = typio_config_get_string(loaded, key.as_ptr(), ptr::null());
        assert!(!got.is_null());
        let s = unsafe { CStr::from_ptr(got) }.to_str().unwrap();
        assert_eq!(s, "persisted");
        typio_config_free(loaded);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn config_load_string() {
        let content = CString::new(r#"key = "from_string""#).unwrap();
        let cfg = typio_config_load_string(content.as_ptr());
        assert!(!cfg.is_null());

        let key = CString::new("key").unwrap();
        let got = typio_config_get_string(cfg, key.as_ptr(), ptr::null());
        assert!(!got.is_null());
        let s = unsafe { CStr::from_ptr(got) }.to_str().unwrap();
        assert_eq!(s, "from_string");

        typio_config_free(cfg);
    }

    #[test]
    fn config_serializes_nested_engine_tables() {
        let cfg = typio_config_new();
        let schema_key = CString::new("engines.rime.schema").unwrap();
        let schema_val = CString::new("m2k_pinyin").unwrap();
        let mode_key = CString::new("engines.compose.printable_key_mode").unwrap();
        let mode_val = CString::new("forward").unwrap();

        typio_config_set_string(cfg, schema_key.as_ptr(), schema_val.as_ptr());
        typio_config_set_string(cfg, mode_key.as_ptr(), mode_val.as_ptr());

        let raw = typio_config_to_string(cfg);
        assert!(!raw.is_null());
        let serialized = unsafe { CStr::from_ptr(raw) }.to_str().unwrap();

        assert!(serialized.contains("[engines.rime]\nschema = \"m2k_pinyin\""));
        assert!(serialized.contains("[engines.compose]\nprintable_key_mode = \"forward\""));
        assert!(!serialized.contains("[engines]\nrime.schema"));

        crate::string::typio_free_string(raw);
        typio_config_free(cfg);
    }

    #[test]
    fn config_key_count() {
        let cfg = typio_config_new();
        assert_eq!(typio_config_key_count(cfg), 0);

        let k1 = CString::new("a").unwrap();
        let v1 = CString::new("1").unwrap();
        typio_config_set_string(cfg, k1.as_ptr(), v1.as_ptr());
        assert_eq!(typio_config_key_count(cfg), 1);

        let k2 = CString::new("b").unwrap();
        let v2 = CString::new("2").unwrap();
        typio_config_set_string(cfg, k2.as_ptr(), v2.as_ptr());
        assert_eq!(typio_config_key_count(cfg), 2);

        typio_config_free(cfg);
    }

    #[test]
    fn config_remove() {
        let cfg = typio_config_new();
        let key = CString::new("temp").unwrap();
        let val = CString::new("value").unwrap();
        typio_config_set_string(cfg, key.as_ptr(), val.as_ptr());
        assert!(typio_config_has_key(cfg, key.as_ptr()));

        let ret = typio_config_remove(cfg, key.as_ptr());
        assert_eq!(ret, TypioResult::TypioOk);
        assert!(!typio_config_has_key(cfg, key.as_ptr()));

        typio_config_free(cfg);
    }

    #[test]
    fn config_default_value() {
        let cfg = typio_config_new();
        let key = CString::new("missing").unwrap();
        let default = CString::new("fallback").unwrap();
        let got = typio_config_get_string(cfg, key.as_ptr(), default.as_ptr());
        assert!(!got.is_null());
        let s = unsafe { CStr::from_ptr(got) }.to_str().unwrap();
        assert_eq!(s, "fallback");
        typio_config_free(cfg);
    }
}
