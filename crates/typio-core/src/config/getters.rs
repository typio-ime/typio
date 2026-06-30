//! C FFI getter functions for configuration values

use super::{Config, ConfigValue};
use std::ffi::{c_char, c_double, c_int, CStr, CString};
use std::ptr;

/// Get a string value from the config, falling back to `default_val`.
#[no_mangle]
pub extern "C" fn typio_config_get_string(
    config: *const Config,
    key: *const c_char,
    default_val: *const c_char,
) -> *const c_char {
    if config.is_null() || key.is_null() {
        return default_val;
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };

    match cfg.entries.get(&*key_str) {
        Some(ConfigValue::String(s)) => s.as_ptr(),
        _ => default_val,
    }
}

/// Get an integer value from the config, falling back to `default_val`.
#[no_mangle]
pub extern "C" fn typio_config_get_int(
    config: *const Config,
    key: *const c_char,
    default_val: c_int,
) -> c_int {
    if config.is_null() || key.is_null() {
        return default_val;
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };

    match cfg.entries.get(&*key_str) {
        Some(ConfigValue::Int(i)) => *i,
        Some(ConfigValue::Float(f)) => *f as c_int,
        _ => default_val,
    }
}

/// Get a boolean value from the config, falling back to `default_val`.
#[no_mangle]
pub extern "C" fn typio_config_get_bool(
    config: *const Config,
    key: *const c_char,
    default_val: bool,
) -> bool {
    if config.is_null() || key.is_null() {
        return default_val;
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };

    match cfg.entries.get(&*key_str) {
        Some(ConfigValue::Bool(b)) => *b,
        _ => default_val,
    }
}

/// Get a floating-point value from the config, falling back to `default_val`.
#[no_mangle]
pub extern "C" fn typio_config_get_float(
    config: *const Config,
    key: *const c_char,
    default_val: c_double,
) -> c_double {
    if config.is_null() || key.is_null() {
        return default_val;
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };

    match cfg.entries.get(&*key_str) {
        Some(ConfigValue::Float(f)) => *f,
        Some(ConfigValue::Int(i)) => *i as c_double,
        _ => default_val,
    }
}

/// Extract a sub-config containing all keys that start with `section.`.
#[no_mangle]
pub extern "C" fn typio_config_get_section(
    config: *const Config,
    section: *const c_char,
) -> *mut Config {
    if config.is_null() || section.is_null() {
        return ptr::null_mut();
    }
    let cfg = unsafe { &*config };
    let section_str = unsafe { CStr::from_ptr(section).to_string_lossy() };
    let prefix = format!("{}.", section_str);

    let mut sub = Config::new();
    for (key, value) in cfg.entries.iter() {
        if key.starts_with(&prefix) {
            let subkey = &key[prefix.len()..];
            sub.entries.insert(subkey.to_string(), value.clone());
        }
    }

    Box::into_raw(Box::new(sub))
}

/// Return the length of an array value, or 0 if missing or not an array.
#[no_mangle]
pub extern "C" fn typio_config_get_array_size(config: *const Config, key: *const c_char) -> usize {
    if config.is_null() || key.is_null() {
        return 0;
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };

    match cfg.entries.get(&*key_str) {
        Some(ConfigValue::Array(arr)) => arr.len(),
        _ => 0,
    }
}

/// Get a string element from an array by index.
#[no_mangle]
pub extern "C" fn typio_config_get_array_string(
    config: *const Config,
    key: *const c_char,
    index: usize,
) -> *const c_char {
    if config.is_null() || key.is_null() {
        return ptr::null();
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };

    match cfg.entries.get(&*key_str) {
        Some(ConfigValue::Array(arr)) if index < arr.len() => match &arr[index] {
            ConfigValue::String(s) => s.as_ptr(),
            _ => ptr::null(),
        },
        _ => ptr::null(),
    }
}

/// Get an integer element from an array by index.
#[no_mangle]
pub extern "C" fn typio_config_get_array_int(
    config: *const Config,
    key: *const c_char,
    index: usize,
) -> c_int {
    if config.is_null() || key.is_null() {
        return 0;
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };

    match cfg.entries.get(&*key_str) {
        Some(ConfigValue::Array(arr)) if index < arr.len() => match &arr[index] {
            ConfigValue::Int(i) => *i,
            _ => 0,
        },
        _ => 0,
    }
}

/// Return the total number of keys in the config.
#[no_mangle]
pub extern "C" fn typio_config_key_count(config: *const Config) -> usize {
    if config.is_null() {
        return 0;
    }
    let cfg = unsafe { &*config };
    cfg.entries.len()
}

/// Return the key name at the given index (caller frees with `free()`).
#[no_mangle]
pub extern "C" fn typio_config_key_at(config: *const Config, index: usize) -> *mut c_char {
    if config.is_null() {
        return ptr::null_mut();
    }
    let cfg = unsafe { &*config };
    let keys: Vec<&String> = cfg.entries.keys().collect();
    if index >= keys.len() {
        return ptr::null_mut();
    }
    // Return an owned, libc-allocated copy (caller frees with free()), matching
    // the typio_strdup string-ownership convention. Using `into_raw()` here
    // returned Rust-allocated memory that callers never reclaimed -> leak.
    match CString::new(keys[index].clone()) {
        Ok(cs) => crate::string::typio_strdup(cs.as_ptr()),
        Err(_) => ptr::null_mut(),
    }
}

/// Return true if the config contains the given key.
#[no_mangle]
pub extern "C" fn typio_config_has_key(config: *const Config, key: *const c_char) -> bool {
    if config.is_null() || key.is_null() {
        return false;
    }
    let cfg = unsafe { &*config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };
    cfg.entries.contains_key(&*key_str)
}
