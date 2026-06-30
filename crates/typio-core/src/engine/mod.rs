//! Engine lifecycle and helper dispatch.
//!
//! Provides C-compatible struct definitions and `#[no_mangle]` entry points
//! used by plugin engines to construct and manage their vtable-based
//! instances. Engine activation, focus routing, and key processing are
//! owned by `core::registry::EngineRegistry`; this module only exposes
//! the construction-time surface engines need.

use crate::string::typio_strdup;
use crate::types::*;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;

/* -------------------------------------------------------------------------- */
/* Engine mode helpers                                                        */
/* -------------------------------------------------------------------------- */

/// Compare two engine modes by identity: mode id (ADR-0011).
pub(crate) fn engine_mode_equal(a: &TypioKeyboardEngineMode, b: &TypioKeyboardEngineMode) -> bool {
    let a_id = unsafe { a.id.as_ref() }.and_then(|p| unsafe { CStr::from_ptr(p) }.to_str().ok());
    let b_id = unsafe { b.id.as_ref() }.and_then(|p| unsafe { CStr::from_ptr(p) }.to_str().ok());
    a_id == b_id
}

/// Deep-copy mode metadata into `dst`, freeing any existing strings first.
pub(crate) fn engine_mode_store(dst: &mut TypioKeyboardEngineMode, src: &TypioKeyboardEngineMode) {
    crate::string::typio_free_string(dst.id as *mut c_char);
    crate::string::typio_free_string(dst.label as *mut c_char);
    crate::string::typio_free_string(dst.display_label as *mut c_char);
    crate::string::typio_free_string(dst.icon_name as *mut c_char);
    crate::string::typio_free_string(dst.profile_id as *mut c_char);
    crate::string::typio_free_string(dst.profile_label as *mut c_char);
    crate::string::typio_free_string(dst.description as *mut c_char);

    dst.id = if src.id.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.id)
    };
    dst.label = if src.label.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.label)
    };
    dst.display_label = if src.display_label.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.display_label)
    };
    dst.icon_name = if src.icon_name.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.icon_name)
    };
    dst.profile_id = if src.profile_id.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.profile_id)
    };
    dst.profile_label = if src.profile_label.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.profile_label)
    };
    dst.description = if src.description.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.description)
    };
    dst.salience = src.salience;
}

/* -------------------------------------------------------------------------- */
/* C ABI: construction (used by plugin engines)                               */
/* -------------------------------------------------------------------------- */

/// Allocate a new `TypioKeyboardEngine` with the given vtables.
///
/// Returns a pointer that must be freed with `typio_engine_free`.
#[no_mangle]
pub extern "C" fn typio_keyboard_engine_new(
    info: *const TypioEngineInfo,
    base_ops: *const TypioEngineBaseOps,
    keyboard: *const TypioKeyboardEngineOps,
) -> *mut TypioKeyboardEngine {
    if info.is_null() || base_ops.is_null() || keyboard.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let engine =
            libc::calloc(1, std::mem::size_of::<TypioKeyboardEngine>()) as *mut TypioKeyboardEngine;
        if engine.is_null() {
            return ptr::null_mut();
        }
        (*engine).base.info = info;
        (*engine).base.base_ops = base_ops;
        (*engine).keyboard = keyboard;
        (*engine).base.active = false;
        (*engine).base.initialized = false;
        engine
    }
}

/// Allocate a new `TypioVoiceEngine` with the given vtables.
///
/// Returns a pointer that must be freed with `typio_engine_free`.
#[no_mangle]
pub extern "C" fn typio_voice_engine_new(
    info: *const TypioEngineInfo,
    base_ops: *const TypioEngineBaseOps,
    voice: *const TypioVoiceEngineOps,
) -> *mut TypioVoiceEngine {
    if info.is_null() || base_ops.is_null() || voice.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let engine =
            libc::calloc(1, std::mem::size_of::<TypioVoiceEngine>()) as *mut TypioVoiceEngine;
        if engine.is_null() {
            return ptr::null_mut();
        }
        (*engine).base.info = info;
        (*engine).base.base_ops = base_ops;
        (*engine).voice = voice;
        (*engine).base.active = false;
        (*engine).base.initialized = false;
        engine
    }
}

/* -------------------------------------------------------------------------- */
/* C ABI: destruction                                                         */
/* -------------------------------------------------------------------------- */

/// Free an engine instance and invoke its `destroy` callback.
#[no_mangle]
pub extern "C" fn typio_engine_free(engine: *mut TypioEngine) {
    if engine.is_null() {
        return;
    }
    unsafe {
        let e = &mut *engine;
        if !e.base_ops.is_null() {
            let base = &*e.base_ops;
            if let Some(destroy) = base.destroy {
                destroy(engine);
            }
        }
        if !e.config_path.is_null() {
            crate::string::typio_free_string(e.config_path);
            e.config_path = ptr::null_mut();
        }
        libc::free(engine as *mut c_void);
    }
}

/* -------------------------------------------------------------------------- */
/* C ABI: getters / setters                                                   */
/* -------------------------------------------------------------------------- */

/// Return the engine's machine-readable name.
#[no_mangle]
pub extern "C" fn typio_engine_get_name(engine: *const TypioEngine) -> *const c_char {
    unsafe {
        if engine.is_null() || (*engine).info.is_null() {
            return ptr::null();
        }
        (*(*engine).info).name
    }
}

/// Return the engine's category.
#[no_mangle]
pub extern "C" fn typio_engine_get_type(engine: *const TypioEngine) -> TypioEngineType {
    unsafe {
        if engine.is_null() || (*engine).info.is_null() {
            return TypioEngineType::TypioEngineTypeKeyboard;
        }
        (*(*engine).info).type_
    }
}

/// Check whether the engine declares the given capability.
#[no_mangle]
pub extern "C" fn typio_engine_has_capability(
    engine: *const TypioEngine,
    capability: *const c_char,
) -> bool {
    unsafe {
        if engine.is_null() || (*engine).info.is_null() || capability.is_null() {
            return false;
        }
        let needle = match CStr::from_ptr(capability).to_str() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let info = &*(*engine).info;
        capability_array_contains(info.required_capabilities, needle)
            || capability_array_contains(info.optional_capabilities, needle)
    }
}

/// Linear scan of a NULL-terminated C-string array for `needle`.
///
/// # Safety
/// `arr` must be NULL or a NULL-terminated array of valid C strings.
unsafe fn capability_array_contains(arr: *const *const c_char, needle: &str) -> bool {
    if arr.is_null() {
        return false;
    }
    let mut cursor = arr;
    while !(*cursor).is_null() {
        if let Ok(s) = CStr::from_ptr(*cursor).to_str() {
            if s == needle {
                return true;
            }
        }
        cursor = cursor.add(1);
    }
    false
}

/// Return true if the engine is currently active.
#[no_mangle]
pub extern "C" fn typio_engine_is_active(engine: *const TypioEngine) -> bool {
    unsafe { !engine.is_null() && (*engine).active }
}

/// Set engine-private user data.
#[no_mangle]
pub extern "C" fn typio_engine_set_user_data(engine: *mut TypioEngine, data: *mut c_void) {
    unsafe {
        if !engine.is_null() {
            (*engine).user_data = data;
        }
    }
}

/// Return engine-private user data.
#[no_mangle]
pub extern "C" fn typio_engine_get_user_data(engine: *const TypioEngine) -> *mut c_void {
    unsafe {
        if engine.is_null() {
            return ptr::null_mut();
        }
        (*engine).user_data
    }
}

/// Set the command surface vtable on an engine.
#[no_mangle]
pub extern "C" fn typio_engine_set_surface_ops(
    engine: *mut TypioEngine,
    ops: *const crate::types::TypioEngineSurfaceOps,
) {
    unsafe {
        if !engine.is_null() {
            (*engine).surface = ops;
        }
    }
}

/// Return the command surface vtable, or NULL.
#[no_mangle]
pub extern "C" fn typio_engine_get_surface_ops(
    engine: *const TypioEngine,
) -> *const crate::types::TypioEngineSurfaceOps {
    unsafe {
        if engine.is_null() {
            return ptr::null();
        }
        (*engine).surface
    }
}

/// Return the engine-specific config file path, or NULL.
#[no_mangle]
pub extern "C" fn typio_engine_get_config_path(engine: *const TypioEngine) -> *const c_char {
    unsafe {
        if engine.is_null() {
            return ptr::null();
        }
        (*engine).config_path
    }
}

/// Set the engine-specific config file path.
///
/// Previous path is freed with `typio_free_string`.
#[no_mangle]
pub extern "C" fn typio_engine_set_config_path(engine: *mut TypioEngine, path: *const c_char) {
    unsafe {
        if engine.is_null() {
            return;
        }
        let e = &mut *engine;
        if !e.config_path.is_null() {
            crate::string::typio_free_string(e.config_path);
        }
        e.config_path = if path.is_null() {
            ptr::null_mut()
        } else {
            typio_strdup(path)
        };
    }
}

/* -------------------------------------------------------------------------- */
/* C ABI: engine command surface (ADR-0008)                                   */
/*                                                                            */
/* Properties were removed from the surface. They live in the unified config  */
/* schema layer; read/write via `typio_config_*`, react via `on_config_change`*/
/* on `TypioEngineBaseOps`.                                                   */
/* -------------------------------------------------------------------------- */

/// List commands exposed by an engine.
///
/// Returns a borrowed pointer into engine-owned storage; do not free.
#[no_mangle]
pub extern "C" fn typio_engine_list_commands(
    engine: *mut TypioEngine,
    out_count: *mut usize,
) -> *const crate::types::TypioEngineCommand {
    unsafe {
        if engine.is_null() || out_count.is_null() {
            return ptr::null();
        }
        let surface = (*engine).surface;
        if surface.is_null() {
            return ptr::null();
        }
        let ops = &*surface;
        if let Some(list_fn) = ops.list_commands {
            return list_fn(engine, out_count);
        }
        ptr::null()
    }
}

/// Invoke a command by id on an engine.
#[no_mangle]
pub extern "C" fn typio_engine_invoke_command(
    engine: *mut TypioEngine,
    id: *const c_char,
) -> TypioResult {
    unsafe {
        if engine.is_null() || id.is_null() {
            return TypioResult::TypioErrorInvalidArgument;
        }
        let surface = (*engine).surface;
        if surface.is_null() {
            return TypioResult::TypioErrorEngineNotAvailable;
        }
        let ops = &*surface;
        if let Some(invoke_fn) = ops.invoke_command {
            return invoke_fn(engine, id);
        }
        TypioResult::TypioErrorEngineNotAvailable
    }
}
