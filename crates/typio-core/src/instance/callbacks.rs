//! Callback registration and notification dispatch

use super::TypioInstance;
use crate::engine::{engine_mode_equal, engine_mode_store};
use crate::types::*;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;

/// Register the engine-changed callback.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_set_engine_changed_callback(
    instance: *mut TypioInstance,
    callback: TypioEngineChangedCallback,
    user_data: *mut c_void,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.callbacks.engine_changed = Some(callback);
    inst.callbacks.engine_changed_user_data = user_data;
}

/// Register the voice-engine-changed callback.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_set_voice_engine_changed_callback(
    instance: *mut TypioInstance,
    callback: TypioVoiceEngineChangedCallback,
    user_data: *mut c_void,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.callbacks.voice_engine_changed = Some(callback);
    inst.callbacks.voice_engine_changed_user_data = user_data;
}

/// Register the status-icon-changed callback.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_set_status_icon_changed_callback(
    instance: *mut TypioInstance,
    callback: TypioStatusIconChangedCallback,
    user_data: *mut c_void,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.callbacks.status_icon_changed = Some(callback);
    inst.callbacks.status_icon_changed_user_data = user_data;
}

/// Notify the host that the status icon has changed.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_notify_status_icon(
    instance: *mut TypioInstance,
    icon_name: *const c_char,
) {
    if instance.is_null() || icon_name.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    let name = unsafe { CStr::from_ptr(icon_name) }.to_string_lossy();
    if inst.last_status_icon.as_ref().map(|s| s.to_str().ok()) == Some(Some(&name)) {
        return;
    }
    inst.last_status_icon = CString::new(name.as_bytes()).ok();
    if let Some(cb) = inst.callbacks.status_icon_changed {
        cb(
            instance.cast(),
            icon_name,
            inst.callbacks.status_icon_changed_user_data,
        );
    }
}

/// Clear the cached status icon.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_clear_status_icon(instance: *mut TypioInstance) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.last_status_icon = None;
}

/// Return the last status icon, or NULL if none is set.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_get_last_status_icon(
    instance: *mut TypioInstance,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    unsafe {
        (*instance)
            .last_status_icon
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null())
    }
}

/// Register the mode-changed callback (host-side).
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_set_keyboard_mode_changed_callback(
    instance: *mut TypioInstance,
    callback: TypioKeyboardModeChangedCallback,
    user_data: *mut c_void,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.callbacks.mode_changed = Some(callback);
    inst.callbacks.mode_changed_user_data = user_data;
}

/// Notify the host that the active engine mode has changed (engine → framework).
///
/// This is the **deliberate** path: the change was a direct result of user
/// input, so the host always confirms it (e.g. flashes the indicator),
/// regardless of salience.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_notify_keyboard_mode(
    instance: *mut TypioInstance,
    mode: *const TypioKeyboardEngineMode,
) {
    if instance.is_null() || mode.is_null() {
        return;
    }
    unsafe { apply_keyboard_mode(instance, &*mode, true) };
}

/// Reconcile the cached active mode and fan out to host callbacks.
///
/// `announce` selects the path:
/// - `true` (deliberate): fire `mode_changed_callback` — the host confirms the
///   change unconditionally (user just acted).
/// - `false` (incidental): refresh `last_mode` and the persistent status icon
///   only. The host's focus path reads `last_mode` and applies its own
///   salience/recency gate, so we must not pre-empt it with a confirmation.
///
/// De-duplicated by mode identity in both paths.
///
/// # Safety
/// `instance` must be a valid `TypioInstance` pointer and `mode_ref` must
/// outlive the call.
pub(crate) unsafe fn apply_keyboard_mode(
    instance: *mut TypioInstance,
    mode_ref: &TypioKeyboardEngineMode,
    announce: bool,
) {
    let inst = &mut *instance;

    if inst.has_mode && engine_mode_equal(&inst.last_mode.0, mode_ref) {
        return;
    }

    engine_mode_store(&mut inst.last_mode.0, mode_ref);
    inst.has_mode = true;

    if !mode_ref.icon_name.is_null() {
        let icon = unsafe { CStr::from_ptr(mode_ref.icon_name) }.to_string_lossy();
        inst.last_status_icon = CString::new(icon.as_bytes()).ok();
    }

    if announce && let Some(cb) = inst.callbacks.mode_changed {
        cb(
            instance.cast(),
            &inst.last_mode.0,
            inst.callbacks.mode_changed_user_data,
        );
    }

    if let Some(cb) = inst.callbacks.status_icon_changed
        && !mode_ref.icon_name.is_null()
    {
        cb(
            instance.cast(),
            mode_ref.icon_name,
            inst.callbacks.status_icon_changed_user_data,
        );
    }
}

/// Reconcile a mode change observed in an out-of-process engine response.
///
/// Bridges the internal [`EngineMode`](crate::core::engine::EngineMode) to the
/// C `TypioKeyboardEngineMode` the host callbacks expect, then dispatches via
/// [`apply_keyboard_mode`]. The temporary `CString`s outlive the call because
/// `apply_keyboard_mode` deep-copies before returning.
///
/// # Safety
/// `instance` must be a valid `TypioInstance` pointer.
pub(crate) unsafe fn dispatch_observed_keyboard_mode(
    instance: *mut TypioInstance,
    mode: &crate::core::engine::EngineMode,
    announce: bool,
) {
    use crate::core::engine::ModeSalience;

    if instance.is_null() || mode.id.is_empty() {
        return;
    }

    let to_cstring = |s: &str| CString::new(s).ok();
    let id = to_cstring(&mode.id);
    let label = to_cstring(&mode.label);
    let display_label = mode.display_label.as_deref().and_then(to_cstring);
    let icon_name = mode.icon.as_deref().and_then(to_cstring);
    let profile_id = mode.profile_id.as_deref().and_then(to_cstring);
    let profile_label = mode.profile_label.as_deref().and_then(to_cstring);
    let description = mode.description.as_deref().and_then(to_cstring);

    let ptr_or_null = |s: &Option<CString>| s.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());

    let c_mode = TypioKeyboardEngineMode {
        id: ptr_or_null(&id),
        label: ptr_or_null(&label),
        display_label: ptr_or_null(&display_label),
        icon_name: ptr_or_null(&icon_name),
        profile_id: ptr_or_null(&profile_id),
        profile_label: ptr_or_null(&profile_label),
        description: ptr_or_null(&description),
        salience: match mode.salience {
            ModeSalience::Notable => TypioStatusSalience::TypioStatusSalienceNotable,
            ModeSalience::Quiet => TypioStatusSalience::TypioStatusSalienceQuiet,
        },
    };

    unsafe { apply_keyboard_mode(instance, &c_mode, announce) };
}

/// Clear the cached mode state.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_clear_keyboard_mode(instance: *mut TypioInstance) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    if !inst.has_mode {
        return;
    }
    crate::string::typio_free_string(inst.last_mode.0.id as *mut c_char);
    crate::string::typio_free_string(inst.last_mode.0.label as *mut c_char);
    crate::string::typio_free_string(inst.last_mode.0.display_label as *mut c_char);
    crate::string::typio_free_string(inst.last_mode.0.icon_name as *mut c_char);
    crate::string::typio_free_string(inst.last_mode.0.profile_id as *mut c_char);
    crate::string::typio_free_string(inst.last_mode.0.profile_label as *mut c_char);
    crate::string::typio_free_string(inst.last_mode.0.description as *mut c_char);
    inst.last_mode = crate::wrappers::InstanceLastMode(TypioKeyboardEngineMode {
        id: ptr::null(),
        label: ptr::null(),
        display_label: ptr::null(),
        icon_name: ptr::null(),
        profile_id: ptr::null(),
        profile_label: ptr::null(),
        description: ptr::null(),
        salience: TypioStatusSalience::TypioStatusSalienceQuiet,
    });
    inst.has_mode = false;
}

/// Return the last mode, or NULL if none is set.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_get_last_keyboard_mode(
    instance: *mut TypioInstance,
) -> *const TypioKeyboardEngineMode {
    if instance.is_null() {
        return ptr::null();
    }
    let inst = unsafe { &*instance };
    if !inst.has_mode {
        return ptr::null();
    }
    &inst.last_mode.0
}

/// Register the engine-availability-changed callback (host-side, ADR-0014).
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_set_engine_availability_changed_callback(
    instance: *mut TypioInstance,
    callback: TypioEngineAvailabilityChangedCallback,
    user_data: *mut c_void,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.callbacks.availability_changed = Some(callback);
    inst.callbacks.availability_changed_user_data = user_data;
}

/// Notify the host that the active engine's availability changed
/// (engine to framework). Caches the state + reason, de-duplicates, and fans out.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_notify_engine_availability(
    instance: *mut TypioInstance,
    state: TypioEngineAvailability,
    reason: *const c_char,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };

    let next_reason = if reason.is_null() {
        None
    } else {
        let r = unsafe { CStr::from_ptr(reason) }.to_string_lossy();
        CString::new(r.as_bytes()).ok()
    };
    let reason_unchanged = inst.last_availability_reason.as_ref().map(|s| s.as_bytes())
        == next_reason.as_ref().map(|s| s.as_bytes());
    if inst.last_availability == state && reason_unchanged {
        return;
    }
    inst.last_availability = state;
    inst.last_availability_reason = next_reason;

    if let Some(cb) = inst.callbacks.availability_changed {
        let reason_ptr = inst
            .last_availability_reason
            .as_ref()
            .map_or(ptr::null(), |s| s.as_ptr());
        cb(
            instance.cast(),
            state,
            reason_ptr,
            inst.callbacks.availability_changed_user_data,
        );
    }
}

/// Return the last notified availability (host pull / initial sync).
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_get_engine_availability(
    instance: *mut TypioInstance,
) -> TypioEngineAvailability {
    if instance.is_null() {
        return TypioEngineAvailability::TypioEngineReady;
    }
    unsafe { (*instance).last_availability }
}

/// Notify the host that the active engine has changed.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_notify_engine_changed(
    instance: *mut TypioInstance,
    engine: *const TypioEngineInfo,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*instance };
    if let Some(cb) = inst.callbacks.engine_changed {
        cb(
            instance.cast(),
            engine,
            inst.callbacks.engine_changed_user_data,
        );
    }
}

/// Notify the host that the active voice engine has changed.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_notify_voice_engine_changed(
    instance: *mut TypioInstance,
    engine: *const TypioEngineInfo,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*instance };
    if let Some(cb) = inst.callbacks.voice_engine_changed {
        cb(
            instance.cast(),
            engine,
            inst.callbacks.voice_engine_changed_user_data,
        );
    }
}

/// Register the languages-changed callback (ADR-0034). Fired whenever an
/// engine updates its declared languages at runtime via
/// `typio_registry_set_engine_languages`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_set_languages_changed_callback(
    instance: *mut TypioInstance,
    callback: TypioLanguagesChangedCallback,
    user_data: *mut c_void,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.callbacks.languages_changed = Some(callback);
    inst.callbacks.languages_changed_user_data = user_data;
}

/// Notify the host that an engine's declared languages changed.
/// `engine_name` is the engine that triggered the change (borrowed, may be
/// NULL to indicate a global refresh). The host re-queries
/// `typio_registry_list_languages` and refreshes derived surfaces.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_notify_languages_changed(
    instance: *mut TypioInstance,
    engine_name: *const c_char,
) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*instance };
    if let Some(cb) = inst.callbacks.languages_changed {
        cb(
            instance.cast(),
            engine_name,
            inst.callbacks.languages_changed_user_data,
        );
    }
}
