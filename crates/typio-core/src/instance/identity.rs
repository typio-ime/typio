//! Per-application identity engine/mode persistence
//!
//! Loads and stores per-app keyboard engine and mode preferences
//! from a TOML file in the instance state directory.

use super::TypioInstance;
use crate::config;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const TYPIO_IDENTITY_STATE_FILE: &str = "identity-engine-state.toml";

/* -------------------------------------------------------------------------- */
/* Internal helpers                                                           */
/* -------------------------------------------------------------------------- */

fn state_path(instance: &TypioInstance) -> Option<std::path::PathBuf> {
    let state_dir = instance.state_dir.as_ref()?;
    let dir = state_dir.to_str().ok()?;
    Some(std::path::PathBuf::from(dir).join(TYPIO_IDENTITY_STATE_FILE))
}

fn hex_encode(text: &str) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(text.len() * 2);
    for &b in text.as_bytes() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn config_key(provider_name: &str, app_id: &str) -> String {
    let stable_key = format!("{}:{}", provider_name, app_id);
    format!("identities.{}", hex_encode(&stable_key))
}

fn config_subkey(provider_name: &str, app_id: &str, suffix: &str) -> String {
    format!("{}{}", config_key(provider_name, app_id), suffix)
}

fn preferences_enabled(instance: *mut TypioInstance) -> bool {
    if instance.is_null() {
        return false;
    }
    let inst = unsafe { &*instance };
    if inst.config.0.is_null() {
        return true;
    }
    let key = CString::new("keyboard.per_app_preferences").unwrap();
    config::typio_config_get_bool(inst.config.0, key.as_ptr(), true)
}

/* -------------------------------------------------------------------------- */
/* C ABI                                                                      */
/* -------------------------------------------------------------------------- */

#[no_mangle]
pub extern "C" fn typio_instance_identity_preferences_enabled(
    instance: *mut TypioInstance,
) -> bool {
    preferences_enabled(instance)
}

#[no_mangle]
pub extern "C" fn typio_instance_identity_load_engine(
    instance: *mut TypioInstance,
    provider_name: *const c_char,
    app_id: *const c_char,
) -> *mut c_char {
    if instance.is_null() || provider_name.is_null() || app_id.is_null() {
        return ptr::null_mut();
    }
    let inst = unsafe { &*instance };
    let provider = unsafe { CStr::from_ptr(provider_name) }
        .to_str()
        .unwrap_or("");
    let app = unsafe { CStr::from_ptr(app_id) }.to_str().unwrap_or("");
    if provider.is_empty() || app.is_empty() {
        return ptr::null_mut();
    }

    let path = match state_path(inst) {
        Some(p) => p,
        None => return ptr::null_mut(),
    };
    let path_c = match CString::new(path.to_string_lossy().as_bytes()) {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let key = CString::new(config_subkey(provider, app, ".engine")).unwrap();

    let cfg = config::typio_config_load_file(path_c.as_ptr());
    if cfg.is_null() {
        return ptr::null_mut();
    }

    let engine_name = unsafe {
        let val = config::typio_config_get_string(cfg, key.as_ptr(), ptr::null());
        if val.is_null() || *val == 0 {
            ptr::null_mut()
        } else {
            libc::strdup(val)
        }
    };

    config::typio_config_free(cfg);
    engine_name
}

#[no_mangle]
pub extern "C" fn typio_instance_identity_store_engine(
    instance: *mut TypioInstance,
    provider_name: *const c_char,
    app_id: *const c_char,
    engine_name: *const c_char,
) {
    if instance.is_null() || provider_name.is_null() || app_id.is_null() || engine_name.is_null() {
        return;
    }
    let inst = unsafe { &*instance };
    let provider = unsafe { CStr::from_ptr(provider_name) }
        .to_str()
        .unwrap_or("");
    let app = unsafe { CStr::from_ptr(app_id) }.to_str().unwrap_or("");
    let engine = unsafe { CStr::from_ptr(engine_name) }
        .to_str()
        .unwrap_or("");
    if provider.is_empty() || app.is_empty() || engine.is_empty() {
        return;
    }

    let path = match state_path(inst) {
        Some(p) => p,
        None => return,
    };
    let path_c = match CString::new(path.to_string_lossy().as_bytes()) {
        Ok(s) => s,
        Err(_) => return,
    };

    let key = CString::new(config_subkey(provider, app, ".engine")).unwrap();
    let mode_engine_key = CString::new(config_subkey(provider, app, ".mode_engine")).unwrap();
    let mode_id_key = CString::new(config_subkey(provider, app, ".mode_id")).unwrap();

    let mut cfg = config::typio_config_load_file(path_c.as_ptr());
    if cfg.is_null() {
        cfg = config::typio_config_new();
    }
    if cfg.is_null() {
        return;
    }

    let stored_mode_engine =
        config::typio_config_get_string(cfg, mode_engine_key.as_ptr(), ptr::null());

    config::typio_config_set_string(cfg, key.as_ptr(), engine_name);

    if !stored_mode_engine.is_null() && unsafe { *stored_mode_engine } != 0 {
        let stored = unsafe { CStr::from_ptr(stored_mode_engine) }
            .to_str()
            .unwrap_or("");
        if stored != engine {
            config::typio_config_remove(cfg, mode_engine_key.as_ptr());
            config::typio_config_remove(cfg, mode_id_key.as_ptr());
        }
    }

    config::typio_config_save_file(cfg, path_c.as_ptr());
    config::typio_config_free(cfg);
}

#[no_mangle]
pub extern "C" fn typio_instance_identity_load_mode(
    instance: *mut TypioInstance,
    provider_name: *const c_char,
    app_id: *const c_char,
    out_engine: *mut *mut c_char,
    out_mode_id: *mut *mut c_char,
) -> bool {
    if !out_engine.is_null() {
        unsafe {
            *out_engine = ptr::null_mut();
        }
    }
    if !out_mode_id.is_null() {
        unsafe {
            *out_mode_id = ptr::null_mut();
        }
    }

    if instance.is_null() || provider_name.is_null() || app_id.is_null() {
        return false;
    }
    let inst = unsafe { &*instance };
    let provider = unsafe { CStr::from_ptr(provider_name) }
        .to_str()
        .unwrap_or("");
    let app = unsafe { CStr::from_ptr(app_id) }.to_str().unwrap_or("");
    if provider.is_empty() || app.is_empty() {
        return false;
    }

    let path = match state_path(inst) {
        Some(p) => p,
        None => return false,
    };
    let path_c = match CString::new(path.to_string_lossy().as_bytes()) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let engine_key = CString::new(config_subkey(provider, app, ".mode_engine")).unwrap();
    let mode_key = CString::new(config_subkey(provider, app, ".mode_id")).unwrap();

    let cfg = config::typio_config_load_file(path_c.as_ptr());
    if cfg.is_null() {
        return false;
    }

    let engine_name = config::typio_config_get_string(cfg, engine_key.as_ptr(), ptr::null());
    let mode_id = config::typio_config_get_string(cfg, mode_key.as_ptr(), ptr::null());

    let loaded = if !engine_name.is_null()
        && unsafe { *engine_name } != 0
        && !mode_id.is_null()
        && unsafe { *mode_id } != 0
    {
        if !out_engine.is_null() {
            unsafe {
                *out_engine = libc::strdup(engine_name);
            }
        }
        if !out_mode_id.is_null() {
            unsafe {
                *out_mode_id = libc::strdup(mode_id);
            }
        }
        true
    } else {
        false
    };

    config::typio_config_free(cfg);
    loaded
}

#[no_mangle]
pub extern "C" fn typio_instance_identity_store_mode(
    instance: *mut TypioInstance,
    provider_name: *const c_char,
    app_id: *const c_char,
    mode_engine: *const c_char,
    mode_id: *const c_char,
) {
    if instance.is_null()
        || provider_name.is_null()
        || app_id.is_null()
        || mode_engine.is_null()
        || mode_id.is_null()
    {
        return;
    }
    let inst = unsafe { &*instance };
    let provider = unsafe { CStr::from_ptr(provider_name) }
        .to_str()
        .unwrap_or("");
    let app = unsafe { CStr::from_ptr(app_id) }.to_str().unwrap_or("");
    if provider.is_empty() || app.is_empty() {
        return;
    }

    let path = match state_path(inst) {
        Some(p) => p,
        None => return,
    };
    let path_c = match CString::new(path.to_string_lossy().as_bytes()) {
        Ok(s) => s,
        Err(_) => return,
    };

    let engine_key = CString::new(config_subkey(provider, app, ".mode_engine")).unwrap();
    let mode_key = CString::new(config_subkey(provider, app, ".mode_id")).unwrap();

    let mut cfg = config::typio_config_load_file(path_c.as_ptr());
    if cfg.is_null() {
        cfg = config::typio_config_new();
    }
    if cfg.is_null() {
        return;
    }

    config::typio_config_set_string(cfg, engine_key.as_ptr(), mode_engine);
    config::typio_config_set_string(cfg, mode_key.as_ptr(), mode_id);
    config::typio_config_save_file(cfg, path_c.as_ptr());
    config::typio_config_free(cfg);
}

#[no_mangle]
pub extern "C" fn typio_instance_identity_clear_mode(
    instance: *mut TypioInstance,
    provider_name: *const c_char,
    app_id: *const c_char,
    current_engine: *const c_char,
) {
    if instance.is_null() || provider_name.is_null() || app_id.is_null() || current_engine.is_null()
    {
        return;
    }
    let inst = unsafe { &*instance };
    let provider = unsafe { CStr::from_ptr(provider_name) }
        .to_str()
        .unwrap_or("");
    let app = unsafe { CStr::from_ptr(app_id) }.to_str().unwrap_or("");
    if provider.is_empty() || app.is_empty() {
        return;
    }

    let path = match state_path(inst) {
        Some(p) => p,
        None => return,
    };
    let path_c = match CString::new(path.to_string_lossy().as_bytes()) {
        Ok(s) => s,
        Err(_) => return,
    };

    let mode_engine_key = CString::new(config_subkey(provider, app, ".mode_engine")).unwrap();
    let mode_id_key = CString::new(config_subkey(provider, app, ".mode_id")).unwrap();

    let cfg = config::typio_config_load_file(path_c.as_ptr());
    if cfg.is_null() {
        return;
    }

    let stored = config::typio_config_get_string(cfg, mode_engine_key.as_ptr(), ptr::null());
    if !stored.is_null() && unsafe { *stored } != 0 {
        let stored_str = unsafe { CStr::from_ptr(stored) }.to_str().unwrap_or("");
        let current = unsafe { CStr::from_ptr(current_engine) }
            .to_str()
            .unwrap_or("");
        if stored_str != current {
            config::typio_config_remove(cfg, mode_engine_key.as_ptr());
            config::typio_config_remove(cfg, mode_id_key.as_ptr());
            config::typio_config_save_file(cfg, path_c.as_ptr());
        }
    }

    config::typio_config_free(cfg);
}
