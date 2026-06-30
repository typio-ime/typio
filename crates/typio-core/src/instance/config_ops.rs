//! Configuration operations — reload, save, get/set config text

use super::{build_config_path, TypioInstance};
use crate::config;
use crate::config_schema;
use crate::types::*;
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

type EngineDirMap = HashMap<String, CString>;

fn typio_instance_get_engine_dir(
    instance: *mut TypioInstance,
    engine_name: *const c_char,
    base_field: fn(&TypioInstance) -> &Option<CString>,
    cache_field: fn(&mut TypioInstance) -> &mut EngineDirMap,
) -> *const c_char {
    if instance.is_null() || engine_name.is_null() {
        return ptr::null();
    }
    let name = unsafe { CStr::from_ptr(engine_name) }.to_string_lossy();
    if name.is_empty() {
        return ptr::null();
    }
    let inst = unsafe { &mut *instance };

    if let Some(cached) = cache_field(inst).get(name.as_ref()) {
        return cached.as_ptr();
    }

    let base = match base_field(inst).as_ref() {
        Some(d) => d.to_string_lossy().into_owned(),
        None => return ptr::null(),
    };

    let path = format!("{}/{}", base, name);
    super::ensure_directory(&path);

    let c_path = match CString::new(path) {
        Ok(s) => s,
        Err(_) => return ptr::null(),
    };

    let name_owned = name.into_owned();
    cache_field(inst).insert(name_owned.clone(), c_path);
    cache_field(inst).get(&name_owned).unwrap().as_ptr()
}

/// Get the configured config directory, or NULL.
#[no_mangle]
pub extern "C" fn typio_instance_get_config_dir(instance: *mut TypioInstance) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    unsafe {
        (*instance)
            .config_dir
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null())
    }
}

/// Get the configured data directory, or NULL.
#[no_mangle]
pub extern "C" fn typio_instance_get_data_dir(instance: *mut TypioInstance) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    unsafe {
        (*instance)
            .data_dir
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null())
    }
}

/// Get the configured state directory, or NULL.
#[no_mangle]
pub extern "C" fn typio_instance_get_state_dir(instance: *mut TypioInstance) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    unsafe {
        (*instance)
            .state_dir
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null())
    }
}

/// Get the engine-scoped data directory, creating it if necessary.
#[no_mangle]
pub extern "C" fn typio_instance_get_engine_data_dir(
    instance: *mut TypioInstance,
    engine_name: *const c_char,
) -> *const c_char {
    typio_instance_get_engine_dir(
        instance,
        engine_name,
        |inst| &inst.data_dir,
        |inst| &mut inst.engine_data_dirs,
    )
}

/// Get the engine-scoped state directory, creating it if necessary.
#[no_mangle]
pub extern "C" fn typio_instance_get_engine_state_dir(
    instance: *mut TypioInstance,
    engine_name: *const c_char,
) -> *const c_char {
    typio_instance_get_engine_dir(
        instance,
        engine_name,
        |inst| &inst.state_dir,
        |inst| &mut inst.engine_state_dirs,
    )
}

/// Get the raw configuration object.
#[no_mangle]
pub extern "C" fn typio_instance_get_config(instance: *mut TypioInstance) -> *mut config::Config {
    if instance.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*instance).config.0 }
}

/// Get the configuration section for the named engine.
#[no_mangle]
pub extern "C" fn typio_instance_get_engine_config(
    instance: *mut TypioInstance,
    engine_name: *const c_char,
) -> *mut config::Config {
    if instance.is_null() || engine_name.is_null() {
        return ptr::null_mut();
    }
    let inst = unsafe { &*instance };
    if inst.config.0.is_null() {
        return ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(engine_name) }.to_string_lossy();
    if name.is_empty() {
        return ptr::null_mut();
    }
    let section = format!("engines.{}", name);
    let section_c = CString::new(section).unwrap();
    config::typio_config_get_section(inst.config.0, section_c.as_ptr())
}

/// Reload configuration from disk and apply defaults.
#[no_mangle]
pub extern "C" fn typio_instance_reload_config(instance: *mut TypioInstance) -> TypioResult {
    if instance.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let inst = unsafe { &mut *instance };

    let config_dir = match inst.config_dir.as_ref() {
        Some(d) => d.to_string_lossy(),
        None => return TypioResult::TypioErrorInvalidArgument,
    };
    let config_path = build_config_path(&config_dir, super::TYPIO_CONFIG_FILE_NAME);
    let path_c = CString::new(config_path).unwrap();

    let new_config = config::typio_config_load_file(path_c.as_ptr());
    if !new_config.is_null() {
        config_schema::typio_config_apply_defaults(new_config);
        if !inst.config.0.is_null() {
            config::typio_config_free(inst.config.0);
        }
        inst.config.0 = new_config;
    }
    if inst.config.0.is_null() {
        inst.config.0 = config::typio_config_new();
        if inst.config.0.is_null() {
            return TypioResult::TypioErrorOutOfMemory;
        }
        config_schema::typio_config_apply_defaults(inst.config.0);
    }

    // Engine config reload is now driven by the registry — the engine's
    // reload_config callback is fired transparently through the backend.
    // Note: default engine activation has been removed; the framework now
    // persists and resumes the last-used engine via engine-state.toml.
    unsafe {
        if !inst.registry.0.is_null() {
            (*inst.registry.0).inner.reload_active_config();
        }
    }

    TypioResult::TypioOk
}

/// Save the current configuration to disk.
#[no_mangle]
pub extern "C" fn typio_instance_save_config(instance: *mut TypioInstance) -> TypioResult {
    if instance.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let inst = unsafe { &*instance };
    inst.save_config()
}

/// Serialize the current configuration to a newly allocated TOML string.
#[no_mangle]
pub extern "C" fn typio_instance_get_config_text(instance: *mut TypioInstance) -> *mut c_char {
    if instance.is_null() {
        return ptr::null_mut();
    }
    let inst = unsafe { &*instance };
    if inst.config.0.is_null() {
        return ptr::null_mut();
    }
    config::typio_config_to_string(inst.config.0)
}

/// Parse the given TOML string and replace the current configuration.
#[no_mangle]
pub extern "C" fn typio_instance_set_config_text(
    instance: *mut TypioInstance,
    content: *const c_char,
) -> TypioResult {
    if instance.is_null() || content.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let inst = unsafe { &mut *instance };

    let parsed = config::typio_config_load_string(content);
    if parsed.is_null() {
        return TypioResult::TypioError;
    }

    let old_key_count = if inst.config.0.is_null() {
        0
    } else {
        config::typio_config_key_count(inst.config.0)
    };
    let new_key_count = config::typio_config_key_count(parsed);
    if old_key_count > 0 && new_key_count == 0 {
        let cstr = unsafe { CStr::from_ptr(content) };
        let mut has_content = false;
        for b in cstr.to_bytes() {
            if !b.is_ascii_whitespace() && *b != b'#' && *b != b';' {
                has_content = true;
                break;
            }
        }
        if !has_content {
            log::warn!(
                "Rejecting empty replacement config while existing config has {} keys",
                old_key_count
            );
            config::typio_config_free(parsed);
            return TypioResult::TypioErrorInvalidArgument;
        }
    }

    config_schema::typio_config_apply_defaults(parsed);

    let old_config = inst.config.0;
    inst.config.0 = parsed;
    let save_result = inst.save_config();
    if save_result != TypioResult::TypioOk {
        inst.config.0 = old_config;
        config::typio_config_free(parsed);
        return save_result;
    }
    if !old_config.is_null() {
        config::typio_config_free(old_config);
    }

    super::typio_instance_reload_config(instance)
}

/// Write an engine-owned config key, persist, and notify the engine.
#[no_mangle]
pub extern "C" fn typio_instance_set_engine_config_key(
    instance: *mut TypioInstance,
    engine_name: *const c_char,
    key: *const c_char,
    value: *const c_char,
) -> TypioResult {
    if instance.is_null() || engine_name.is_null() || key.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let inst = unsafe { &*instance };
    if inst.config.0.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }

    let name_str = unsafe { CStr::from_ptr(engine_name) }.to_string_lossy();
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    let full_key = format!("engines.{}.{}", name_str, key_str);

    let full_key_c = CString::new(full_key.clone()).unwrap();
    let field = config_schema::typio_config_schema_find(full_key_c.as_ptr());
    if field.is_null() {
        log::warn!("Engine config key '{}' not found in schema", full_key);
        return TypioResult::TypioErrorNotFound;
    }

    let val_str = if value.is_null() {
        std::borrow::Cow::Borrowed("")
    } else {
        unsafe { CStr::from_ptr(value) }.to_string_lossy()
    };
    let val_c = CString::new(val_str.as_ref()).unwrap();
    let result =
        config::typio_config_set_string(inst.config.0, full_key_c.as_ptr(), val_c.as_ptr());
    if result != TypioResult::TypioOk {
        return result;
    }

    let save_result = inst.save_config();
    if save_result != TypioResult::TypioOk {
        return save_result;
    }

    let registry = inst.registry.0;
    if !registry.is_null() {
        let reg = unsafe { &mut *registry };
        let name_owned = name_str.into_owned();
        let full_key_owned = full_key;
        let val_owned = val_str.into_owned();
        let _ = reg
            .inner
            .notify_config_change(&name_owned, &full_key_owned, &val_owned);
    }

    TypioResult::TypioOk
}
