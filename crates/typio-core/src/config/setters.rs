//! C FFI setter functions for configuration values

use super::{Config, ConfigValue};
use crate::types::*;
use std::ffi::{CStr, CString, c_char, c_double, c_int};

/// Set a string value for a key.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_set_string(
    config: *mut Config,
    key: *const c_char,
    value: *const c_char,
) -> TypioResult {
    if config.is_null() || key.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &mut *config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };
    let val_str = if value.is_null() {
        CString::default()
    } else {
        unsafe { CStr::from_ptr(value).to_owned() }
    };
    cfg.set_value(key_str, ConfigValue::String(val_str));
    TypioResult::TypioOk
}

/// Set an integer value for a key.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_set_int(
    config: *mut Config,
    key: *const c_char,
    value: c_int,
) -> TypioResult {
    if config.is_null() || key.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &mut *config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };
    cfg.set_value(key_str, ConfigValue::Int(value));
    TypioResult::TypioOk
}

/// Set a boolean value for a key.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_set_bool(
    config: *mut Config,
    key: *const c_char,
    value: bool,
) -> TypioResult {
    if config.is_null() || key.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &mut *config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };
    cfg.set_value(key_str, ConfigValue::Bool(value));
    TypioResult::TypioOk
}

/// Set a floating-point value for a key.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_set_float(
    config: *mut Config,
    key: *const c_char,
    value: c_double,
) -> TypioResult {
    if config.is_null() || key.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &mut *config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };
    cfg.set_value(key_str, ConfigValue::Float(value));
    TypioResult::TypioOk
}

/// Set an array of strings for a key.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_set_string_array(
    config: *mut Config,
    key: *const c_char,
    values: *const *const c_char,
    count: usize,
) -> TypioResult {
    if config.is_null() || key.is_null() || (values.is_null() && count > 0) {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &mut *config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };

    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        let ptr = unsafe { *values.add(i) };
        let s = if ptr.is_null() {
            CString::default()
        } else {
            unsafe { CStr::from_ptr(ptr).to_owned() }
        };
        items.push(ConfigValue::String(s));
    }

    cfg.set_value(key_str, ConfigValue::Array(items));
    TypioResult::TypioOk
}

/// Merge all entries from `sub_config` under `section.` prefix.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_set_section(
    config: *mut Config,
    section: *const c_char,
    sub_config: *const Config,
) -> TypioResult {
    if config.is_null() || section.is_null() || sub_config.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &mut *config };
    let section_str = unsafe { CStr::from_ptr(section).to_string_lossy() };
    let sub = unsafe { &*sub_config };

    for (key, value) in sub.entries.iter() {
        let full_key = format!("{}.{}", section_str, key);
        cfg.set_value(full_key, value.clone());
    }

    TypioResult::TypioOk
}

/// Remove a key from the config.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_remove(config: *mut Config, key: *const c_char) -> TypioResult {
    if config.is_null() || key.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let cfg = unsafe { &mut *config };
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };
    cfg.user_keys.remove(&key_str);
    if cfg.entries.remove(&key_str).is_some() {
        TypioResult::TypioOk
    } else {
        TypioResult::TypioErrorNotFound
    }
}

/// Copy all entries from `src` into `dest`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_merge(dest: *mut Config, src: *const Config) -> TypioResult {
    if dest.is_null() || src.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let dst = unsafe { &mut *dest };
    let s = unsafe { &*src };

    for (key, value) in s.entries.iter() {
        if s.user_keys.contains(key) {
            dst.set_value(key.clone(), value.clone());
        } else {
            dst.set_default_value(key.clone(), value.clone());
        }
    }

    TypioResult::TypioOk
}
