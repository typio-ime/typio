//! C ABI surface for the engine registry (ADR-0005).
//!
//! `typio_registry_*` is the sole engine-management surface exposed to C
//! callers.
//!
//! Engines are registered as out-of-process engine-process backends. There is no
//! in-process plugin registration path.

use crate::core::engine::backend::{process::ProcessBackend, EngineBackend};
use crate::core::engine::{EngineAvailability, EngineInfo, EngineType};
use crate::core::registry::{EngineRegistry, SwitchDirection};
use crate::types::*;
use crate::TypioInstance;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

/* -------------------------------------------------------------------------- */
/* TypioRegistry opaque handle                                                */
/* -------------------------------------------------------------------------- */

/// Opaque registry handle exposed across the C ABI.
#[repr(C)]
pub struct TypioRegistry {
    pub(crate) inner: EngineRegistry,
    pub(crate) instance: *mut TypioInstance,
}

/* -------------------------------------------------------------------------- */
/* Helpers                                                                    */
/* -------------------------------------------------------------------------- */

/// Convert a NULL-terminated array of NUL-terminated C strings to `Vec<String>`.
/// Safe to call with a NULL outer pointer (returns empty vec).
///
/// # Safety
/// `arr` must either be NULL or point to a NULL-terminated array whose
/// non-NULL entries are valid NUL-terminated C strings.
pub(crate) unsafe fn c_str_array_to_vec(arr: *const *const c_char) -> Vec<String> {
    if arr.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = arr;
    while !(*cursor).is_null() {
        out.push(CStr::from_ptr(*cursor).to_string_lossy().into_owned());
        cursor = cursor.add(1);
    }
    out
}

/// Validate an engine icon string.
///
/// Returns `Some(icon)` if valid, `None` if invalid (and logs a warning).
fn validate_engine_icon(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        log::warn!("Engine icon rejected: empty or whitespace-only");
        return None;
    }
    if trimmed.contains("://") {
        log::warn!(
            "Engine icon rejected: URL scheme not allowed ({:?})",
            trimmed
        );
        return None;
    }
    if trimmed.contains("..") {
        log::warn!(
            "Engine icon rejected: parent-directory reference not allowed ({:?})",
            trimmed
        );
        return None;
    }
    if trimmed.starts_with('~') {
        log::warn!(
            "Engine icon rejected: home-directory reference not allowed ({:?})",
            trimmed
        );
        return None;
    }
    if trimmed.starts_with('/') {
        log::warn!(
            "Engine icon absolute paths are deprecated ({:?}); \
             use freedesktop icon names and bundle icons in <engine-dir>/icons/",
            trimmed
        );
    }
    Some(trimmed.to_string())
}

fn map_engine_availability(state: EngineAvailability) -> TypioEngineAvailability {
    match state {
        EngineAvailability::Uninitialized => TypioEngineAvailability::TypioEngineUninitialized,
        EngineAvailability::Preparing => TypioEngineAvailability::TypioEnginePreparing,
        EngineAvailability::Ready => TypioEngineAvailability::TypioEngineReady,
        EngineAvailability::Failed => TypioEngineAvailability::TypioEngineFailed,
    }
}

/// Build a complete `EngineInfo` from a C `TypioEngineInfo`.
fn engine_info_from_c(c_info: &TypioEngineInfo) -> EngineInfo {
    let engine_type = match c_info.type_ {
        TypioEngineType::TypioEngineTypeKeyboard => EngineType::Keyboard,
        TypioEngineType::TypioEngineTypeVoice => EngineType::Voice,
        _ => EngineType::Keyboard,
    };
    let name = unsafe { CStr::from_ptr(c_info.name) }
        .to_string_lossy()
        .into_owned();
    let mut info = EngineInfo::new(name, engine_type);
    if !c_info.display_name.is_null() {
        info.display_name = unsafe { CStr::from_ptr(c_info.display_name) }
            .to_string_lossy()
            .into_owned();
    }
    if !c_info.description.is_null() {
        info.description = unsafe { CStr::from_ptr(c_info.description) }
            .to_string_lossy()
            .into_owned();
    }
    if !c_info.author.is_null() {
        info.author = unsafe { CStr::from_ptr(c_info.author) }
            .to_string_lossy()
            .into_owned();
    }
    if !c_info.icon.is_null() {
        let raw_icon = unsafe { CStr::from_ptr(c_info.icon) }
            .to_string_lossy()
            .into_owned();
        info.icon = validate_engine_icon(&raw_icon);
    }
    if !c_info.language.is_null() {
        info.language = unsafe { CStr::from_ptr(c_info.language) }
            .to_string_lossy()
            .into_owned();
    }
    info.capabilities = crate::core::engine::EngineCapabilities {
        required: unsafe { c_str_array_to_vec(c_info.required_capabilities) },
        optional: unsafe { c_str_array_to_vec(c_info.optional_capabilities) },
    };
    info
}

/// Build a heap-allocated NULL-terminated C-string array from a slice of strings.
/// Returns NULL when the input is empty so the consumer can short-circuit.
fn alloc_c_str_array(items: &[String]) -> *const *const c_char {
    if items.is_empty() {
        return ptr::null();
    }
    let mut raw: Vec<*const c_char> = Vec::with_capacity(items.len() + 1);
    for s in items {
        raw.push(CString::new(s.as_str()).unwrap_or_default().into_raw() as *const c_char);
    }
    raw.push(ptr::null());
    let boxed = raw.into_boxed_slice();
    Box::into_raw(boxed) as *const *const c_char
}

/// Free a NULL-terminated array previously returned by `alloc_c_str_array`,
/// including all the inner strings.
unsafe fn free_c_str_array(arr: *const *const c_char) {
    if arr.is_null() {
        return;
    }
    let mut len = 0usize;
    while !(*arr.add(len)).is_null() {
        drop(CString::from_raw(*arr.add(len) as *mut c_char));
        len += 1;
    }
    // +1 for the trailing NULL sentinel.
    let slice = std::ptr::slice_from_raw_parts_mut(arr as *mut *const c_char, len + 1);
    drop(Box::from_raw(slice));
}

/// Build a freshly-allocated `TypioEngineInfo` from an internal `EngineInfo`.
///
/// Caller must release with [`typio_engine_info_free`].
fn alloc_c_engine_info(info: &EngineInfo) -> *const TypioEngineInfo {
    let c_info = TypioEngineInfo {
        name: CString::new(info.name.as_str())
            .unwrap_or_default()
            .into_raw(),
        display_name: if info.display_name.is_empty() {
            ptr::null()
        } else {
            CString::new(info.display_name.as_str())
                .unwrap_or_default()
                .into_raw()
        },
        description: if info.description.is_empty() {
            ptr::null()
        } else {
            CString::new(info.description.as_str())
                .unwrap_or_default()
                .into_raw()
        },
        author: if info.author.is_empty() {
            ptr::null()
        } else {
            CString::new(info.author.as_str())
                .unwrap_or_default()
                .into_raw()
        },
        icon: match info.icon.as_ref() {
            Some(icon) if !icon.is_empty() => {
                CString::new(icon.as_str()).unwrap_or_default().into_raw()
            }
            _ => ptr::null(),
        },
        language: CString::new(info.language.as_str())
            .unwrap_or_default()
            .into_raw(),
        type_: match info.engine_type {
            EngineType::Keyboard => TypioEngineType::TypioEngineTypeKeyboard,
            EngineType::Voice => TypioEngineType::TypioEngineTypeVoice,
        },
        required_capabilities: alloc_c_str_array(&info.capabilities.required),
        optional_capabilities: alloc_c_str_array(&info.capabilities.optional),
    };
    Box::into_raw(Box::new(c_info))
}

/// Fire the engine-changed callback registered on the instance, if any.
unsafe fn notify_keyboard_changed(registry: &TypioRegistry) {
    if registry.instance.is_null() {
        return;
    }
    let name = registry.inner.active_keyboard_name().map(|s| s.to_string());
    let info_ptr = match name {
        Some(ref n) => registry
            .inner
            .engine_info(n)
            .map(alloc_c_engine_info)
            .unwrap_or(ptr::null()),
        None => ptr::null(),
    };
    crate::instance::typio_instance_notify_engine_changed(registry.instance, info_ptr);
    crate::instance::typio_instance_notify_engine_availability(
        registry.instance,
        map_engine_availability(registry.inner.active_keyboard_availability()),
        ptr::null(),
    );
    if !info_ptr.is_null() {
        typio_engine_info_free(info_ptr as *mut _);
    }
}

unsafe fn notify_voice_changed(registry: &TypioRegistry) {
    if registry.instance.is_null() {
        return;
    }
    let name = registry.inner.active_voice_name().map(|s| s.to_string());
    let info_ptr = match name {
        Some(ref n) => registry
            .inner
            .engine_info(n)
            .map(alloc_c_engine_info)
            .unwrap_or(ptr::null()),
        None => ptr::null(),
    };
    crate::instance::typio_instance_notify_voice_engine_changed(registry.instance, info_ptr);
    crate::instance::typio_instance_notify_engine_availability(
        registry.instance,
        map_engine_availability(registry.inner.active_voice_availability()),
        ptr::null(),
    );
    if !info_ptr.is_null() {
        typio_engine_info_free(info_ptr as *mut _);
    }
}

/* -------------------------------------------------------------------------- */
/* Lifecycle                                                                  */
/* -------------------------------------------------------------------------- */

/// Create a new engine registry tied to the given instance.
///
/// Returns a pointer that must be freed with `typio_registry_free`.
#[no_mangle]
pub extern "C" fn typio_registry_new(instance: *mut TypioInstance) -> *mut TypioRegistry {
    let inner = EngineRegistry::new();
    Box::into_raw(Box::new(TypioRegistry { inner, instance }))
}

/// Free a registry previously created by `typio_registry_new`.
#[no_mangle]
pub extern "C" fn typio_registry_free(registry: *mut TypioRegistry) {
    if registry.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(registry));
    }
}

/// Return the parent instance, or NULL.
#[no_mangle]
pub extern "C" fn typio_registry_get_instance(registry: *mut TypioRegistry) -> *mut TypioInstance {
    if registry.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*registry).instance }
}

/* -------------------------------------------------------------------------- */
/* ABI version negotiation                                                    */
/* -------------------------------------------------------------------------- */

/// Validate an engine's reported ABI version against this runtime build.
///
/// Direct workers link `typio_engine_abi_version` into the executable; a
/// compatibility loader may resolve it with `dlsym` inside the worker process.
/// Callers pass its result here before registering the engine. Returns `true`
/// only when the major version matches exactly and the engine's minor does not
/// exceed the runtime's.
///
/// # Safety
/// `plugin` must be NULL or point to a valid `TypioAbiVersion`.
#[no_mangle]
pub unsafe extern "C" fn typio_engine_abi_check(plugin: *const TypioAbiVersion) -> bool {
    if plugin.is_null() {
        return false;
    }
    let v = unsafe { &*plugin };
    v.major == typio_abi::TYPIO_ENGINE_ABI_MAJOR && v.minor <= typio_abi::TYPIO_ENGINE_ABI_MINOR
}

/* -------------------------------------------------------------------------- */
/* Engine process registration                                                */
/* -------------------------------------------------------------------------- */

/// Register an out-of-process engine process.
///
/// `info` is copied immediately. `argv` is a NULL-terminated argument vector;
/// `argv[0]` is the executable path and remaining entries are passed as
/// arguments when the engine is activated.
#[no_mangle]
pub extern "C" fn typio_registry_register_engine_process(
    registry: *mut TypioRegistry,
    info: *const TypioEngineInfo,
    argv: *const *const c_char,
) -> TypioResult {
    if registry.is_null() || info.is_null() || argv.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let c_info = unsafe { &*info };
    if c_info.name.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let argv = unsafe { c_str_array_to_vec(argv) };
    if argv.is_empty() {
        return TypioResult::TypioErrorInvalidArgument;
    }

    let engine_info = engine_info_from_c(c_info);
    let backend = ProcessBackend::new(engine_info, argv);
    let reg = unsafe { &mut (*registry).inner };
    match reg.register(EngineBackend::Process(backend)) {
        Ok(()) => TypioResult::TypioOk,
        Err(crate::core::engine::EngineError::AlreadyExists) => {
            TypioResult::TypioErrorAlreadyExists
        }
        Err(_) => TypioResult::TypioErrorEngineLoadFailed,
    }
}

/* -------------------------------------------------------------------------- */
/* Unload                                                                     */
/* -------------------------------------------------------------------------- */

/// Unload and unregister an engine by name.
#[no_mangle]
pub extern "C" fn typio_registry_unload(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> TypioResult {
    if registry.is_null() || name.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let reg = unsafe { &mut (*registry).inner };
    match reg.unregister(&name_str) {
        Ok(()) => TypioResult::TypioOk,
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/* -------------------------------------------------------------------------- */
/* Listing                                                                    */
/* -------------------------------------------------------------------------- */

fn names_to_c_array(names: Vec<&str>, count: *mut usize) -> *mut *mut c_char {
    unsafe { *count = names.len() };
    if names.is_empty() {
        return ptr::null_mut();
    }
    let ptrs: Vec<*mut c_char> = names
        .into_iter()
        .map(|n| CString::new(n).unwrap_or_default().into_raw())
        .collect();
    Box::into_raw(ptrs.into_boxed_slice()) as *mut *mut c_char
}

/// List registered keyboard engine names.
///
/// Returns a NULL-terminated array of freshly allocated strings.
/// Caller must free each string and the outer array.
#[no_mangle]
pub extern "C" fn typio_registry_list_keyboards(
    registry: *mut TypioRegistry,
    count: *mut usize,
) -> *mut *mut c_char {
    if registry.is_null() || count.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    names_to_c_array(reg.list_keyboards(), count)
}

/// List registered voice engine names.
///
/// Returns a NULL-terminated array of freshly allocated strings.
/// Caller must free each string and the outer array.
#[no_mangle]
pub extern "C" fn typio_registry_list_voices(
    registry: *mut TypioRegistry,
    count: *mut usize,
) -> *mut *mut c_char {
    if registry.is_null() || count.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    names_to_c_array(reg.list_voices(), count)
}

/// Return keyboard engine names in the registry's preferred order.
///
/// Intended to honour the `engine_order` config key once the registry gains
/// access to the instance config; until then this is equivalent to
/// [`typio_registry_list_keyboards`] (registration order). Callers that
/// already hold a config-resolved ordering should use that directly.
#[no_mangle]
pub extern "C" fn typio_registry_list_ordered_keyboards(
    registry: *mut TypioRegistry,
    count: *mut usize,
) -> *mut *mut c_char {
    typio_registry_list_keyboards(registry, count)
}

/* -------------------------------------------------------------------------- */
/* Engine info (full snapshot)                                                */
/* -------------------------------------------------------------------------- */

/// Return a fresh `TypioEngineInfo` for the named engine, or NULL.
/// Caller must release with `typio_engine_info_free`.
#[no_mangle]
pub extern "C" fn typio_registry_get_engine_info(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> *const TypioEngineInfo {
    if registry.is_null() || name.is_null() {
        return ptr::null();
    }
    let reg = unsafe { &(*registry).inner };
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    match reg.engine_info(&name_str) {
        Some(info) => alloc_c_engine_info(info),
        None => ptr::null(),
    }
}

/// Release a `TypioEngineInfo` previously returned by
/// `typio_registry_get_engine_info`, including all interior strings.
#[no_mangle]
pub extern "C" fn typio_engine_info_free(info: *mut TypioEngineInfo) {
    if info.is_null() {
        return;
    }
    unsafe {
        let c_info = Box::from_raw(info);
        for field in [
            c_info.name,
            c_info.display_name,
            c_info.description,
            c_info.author,
            c_info.icon,
            c_info.language,
        ] {
            if !field.is_null() {
                drop(CString::from_raw(field as *mut c_char));
            }
        }
        free_c_str_array(c_info.required_capabilities);
        free_c_str_array(c_info.optional_capabilities);
    }
}

/* -------------------------------------------------------------------------- */
/* Individual metadata getters (return strings owned by libtypio)             */
/* -------------------------------------------------------------------------- */

fn strdup_engine_field<F>(registry: *mut TypioRegistry, name: *const c_char, f: F) -> *mut c_char
where
    F: FnOnce(&EngineInfo) -> &str,
{
    if registry.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    match reg.engine_info(&name_str) {
        Some(info) => {
            let s = f(info);
            if s.is_empty() {
                ptr::null_mut()
            } else {
                CString::new(s).unwrap_or_default().into_raw()
            }
        }
        None => ptr::null_mut(),
    }
}

/// Return the display name of the named engine, or NULL.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_engine_display_name(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> *mut c_char {
    strdup_engine_field(registry, name, |info| &info.display_name)
}

/// Return the icon name of the named engine, or NULL.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_engine_icon(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> *mut c_char {
    if registry.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    match reg.engine_info(&name_str) {
        Some(info) => match &info.icon {
            Some(icon) => CString::new(icon.as_str()).unwrap_or_default().into_raw(),
            None => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    }
}

/// Return the description of the named engine, or NULL.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_engine_description(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> *mut c_char {
    strdup_engine_field(registry, name, |info| &info.description)
}

/// Return the author of the named engine, or NULL.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_engine_author(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> *mut c_char {
    strdup_engine_field(registry, name, |info| &info.author)
}

/// Return the BCP-47 language tag of the named engine, or NULL.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_engine_language(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> *mut c_char {
    strdup_engine_field(registry, name, |info| &info.language)
}

/* -------------------------------------------------------------------------- */
/* Activation / Switching                                                     */
/* -------------------------------------------------------------------------- */

/// Activate the named keyboard engine.
///
/// Returns `TypioErrorNotFound` if the engine is not registered.
#[no_mangle]
pub extern "C" fn typio_registry_set_active_keyboard(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> TypioResult {
    if registry.is_null() || name.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let reg = unsafe { &mut *registry };
    match reg.inner.activate_keyboard(&name_str) {
        Ok(()) => {
            unsafe { notify_keyboard_changed(reg) };
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/// Activate the named voice engine.
///
/// Returns `TypioErrorNotFound` if the engine is not registered.
#[no_mangle]
pub extern "C" fn typio_registry_set_active_voice(
    registry: *mut TypioRegistry,
    name: *const c_char,
) -> TypioResult {
    if registry.is_null() || name.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let reg = unsafe { &mut *registry };
    match reg.inner.activate_voice(&name_str) {
        Ok(()) => {
            unsafe { notify_voice_changed(reg) };
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/// Return the name of the currently active keyboard engine, or NULL.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_active_keyboard(registry: *mut TypioRegistry) -> *mut c_char {
    if registry.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    match reg.active_keyboard_name() {
        Some(name) => CString::new(name).unwrap_or_default().into_raw(),
        None => ptr::null_mut(),
    }
}

/// Return the name of the currently active voice engine, or NULL.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_active_voice(registry: *mut TypioRegistry) -> *mut c_char {
    if registry.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    match reg.active_voice_name() {
        Some(name) => CString::new(name).unwrap_or_default().into_raw(),
        None => ptr::null_mut(),
    }
}

/// Return the active keyboard engine availability.
#[no_mangle]
pub extern "C" fn typio_registry_get_active_keyboard_availability(
    registry: *mut TypioRegistry,
) -> TypioEngineAvailability {
    if registry.is_null() {
        return TypioEngineAvailability::TypioEngineFailed;
    }
    let reg = unsafe { &(*registry).inner };
    map_engine_availability(reg.active_keyboard_availability())
}

/// Return the active voice engine availability.
#[no_mangle]
pub extern "C" fn typio_registry_get_active_voice_availability(
    registry: *mut TypioRegistry,
) -> TypioEngineAvailability {
    if registry.is_null() {
        return TypioEngineAvailability::TypioEngineFailed;
    }
    let reg = unsafe { &(*registry).inner };
    map_engine_availability(reg.active_voice_availability())
}

/// Switch to the next keyboard engine in the ordered list.
#[no_mangle]
pub extern "C" fn typio_registry_next_keyboard(registry: *mut TypioRegistry) -> TypioResult {
    if registry.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut *registry };
    match reg.inner.switch_keyboard(SwitchDirection::Next) {
        Ok(()) => {
            unsafe { notify_keyboard_changed(reg) };
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/// Switch to the previous keyboard engine in the ordered list.
#[no_mangle]
pub extern "C" fn typio_registry_prev_keyboard(registry: *mut TypioRegistry) -> TypioResult {
    if registry.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut *registry };
    match reg.inner.switch_keyboard(SwitchDirection::Previous) {
        Ok(()) => {
            unsafe { notify_keyboard_changed(reg) };
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/// Switch to the next voice engine in the ordered list.
#[no_mangle]
pub extern "C" fn typio_registry_next_voice(registry: *mut TypioRegistry) -> TypioResult {
    if registry.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut *registry };
    match reg.inner.switch_voice(SwitchDirection::Next) {
        Ok(()) => {
            unsafe { notify_voice_changed(reg) };
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/// Switch to the previous voice engine in the ordered list.
#[no_mangle]
pub extern "C" fn typio_registry_prev_voice(registry: *mut TypioRegistry) -> TypioResult {
    if registry.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut *registry };
    match reg.inner.switch_voice(SwitchDirection::Previous) {
        Ok(()) => {
            unsafe { notify_voice_changed(reg) };
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/* -------------------------------------------------------------------------- */
/* Language model (ADR-0018)                                                  */
/* -------------------------------------------------------------------------- */

fn registry_config(reg: &TypioRegistry) -> Option<&crate::config::Config> {
    if reg.instance.is_null() {
        return None;
    }
    let cfg = crate::instance::typio_instance_get_config(reg.instance);
    if cfg.is_null() {
        None
    } else {
        Some(unsafe { &*cfg })
    }
}

/// Enabled language cycle: the `languages.enabled` config key (array or
/// comma-separated string), falling back to every engine-declared language.
fn enabled_languages(reg: &TypioRegistry) -> Vec<String> {
    use crate::config::ConfigValue;
    if let Some(cfg) = registry_config(reg) {
        let list: Vec<String> = match cfg.entries.get("languages.enabled") {
            Some(ConfigValue::Array(items)) => items
                .iter()
                .filter_map(|i| match i {
                    ConfigValue::String(s) => s
                        .to_str()
                        .ok()
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(str::to_string),
                    _ => None,
                })
                .collect(),
            Some(ConfigValue::String(s)) => s
                .to_str()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        };
        if !list.is_empty() {
            return list;
        }
    }
    reg.inner.known_languages()
}

/// Per-language engine override: `languages.<tag>.<modality>`.
fn language_override(reg: &TypioRegistry, tag: &str, modality: &str) -> Option<String> {
    use crate::config::ConfigValue;
    let cfg = registry_config(reg)?;
    match cfg.entries.get(&format!("languages.{}.{}", tag, modality)) {
        Some(ConfigValue::String(s)) => s.to_str().ok().map(str::to_string),
        _ => None,
    }
}

fn activate_language_with_config(reg: &mut TypioRegistry, tag: &str) -> TypioResult {
    let keyboard = language_override(reg, tag, "keyboard");
    let voice = language_override(reg, tag, "voice");
    match reg
        .inner
        .activate_language(tag, keyboard.as_deref(), voice.as_deref())
    {
        Ok(()) => {
            unsafe {
                notify_keyboard_changed(reg);
                notify_voice_changed(reg);
            }
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::InvalidArgument) => {
            TypioResult::TypioErrorInvalidArgument
        }
        Err(_) => TypioResult::TypioError,
    }
}

/// Replace the declared language list of a registered engine.
///
/// `languages` is a NULL-terminated array of BCP-47 tags, primary first.
/// Hosts call this right after registration with the manifest's `languages`
/// value. Returns `TypioErrorNotFound` for an unknown engine.
#[no_mangle]
pub extern "C" fn typio_registry_set_engine_languages(
    registry: *mut TypioRegistry,
    name: *const c_char,
    languages: *const *const c_char,
) -> TypioResult {
    if registry.is_null() || name.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let name_str = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    let name_c = CString::new(name_str.as_bytes()).unwrap_or_default();
    let langs = unsafe { c_str_array_to_vec(languages) };
    let reg = unsafe { &mut *registry };
    match reg.inner.set_engine_languages(&name_str, langs) {
        Ok(()) => {
            /* ADR-0034: dynamic engine capabilities. Fire the
             * languages-changed callback so the host can rebuild the language
             * menu and validate the active language. The name pointer passed
             * to the callback is borrowed from `name_c` and valid only for
             * the call. */
            if !reg.instance.is_null() {
                crate::instance::typio_instance_notify_languages_changed(
                    reg.instance,
                    name_c.as_ptr(),
                );
            }
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioErrorInvalidArgument,
    }
}

/// Return the declared language list of the named engine.
///
/// Caller must release with `typio_free_string_array(list, count)`.
#[no_mangle]
pub extern "C" fn typio_registry_get_engine_languages(
    registry: *mut TypioRegistry,
    name: *const c_char,
    count: *mut usize,
) -> *mut *mut c_char {
    if registry.is_null() || name.is_null() || count.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    match reg.engine_info(&name_str) {
        Some(info) => names_to_c_array(
            info.effective_languages()
                .iter()
                .map(String::as_str)
                .collect(),
            count,
        ),
        None => {
            unsafe { *count = 0 };
            ptr::null_mut()
        }
    }
}

/// List the enabled language cycle (`languages.enabled`, falling back to
/// every engine-declared language in registration order).
///
/// Caller must release with `typio_free_string_array(list, count)`.
#[no_mangle]
pub extern "C" fn typio_registry_list_languages(
    registry: *mut TypioRegistry,
    count: *mut usize,
) -> *mut *mut c_char {
    if registry.is_null() || count.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &*registry };
    let langs = enabled_languages(reg);
    names_to_c_array(langs.iter().map(String::as_str).collect(), count)
}

/// Return the active language tag, or NULL when no language was activated.
///
/// Caller must free the returned string.
#[no_mangle]
pub extern "C" fn typio_registry_get_active_language(registry: *mut TypioRegistry) -> *mut c_char {
    if registry.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &(*registry).inner };
    match reg.active_language() {
        Some(tag) => CString::new(tag).unwrap_or_default().into_raw(),
        None => ptr::null_mut(),
    }
}

/// Activate a language: re-resolve and retarget every modality slot.
///
/// Engine choice per modality follows `languages.<tag>.keyboard` /
/// `languages.<tag>.voice` (the string `"none"` forces an empty slot), then
/// the first registered engine declaring a matching language. A modality
/// with no engine is deactivated; for keyboards this yields raw passthrough
/// (layout-only languages).
#[no_mangle]
pub extern "C" fn typio_registry_set_active_language(
    registry: *mut TypioRegistry,
    tag: *const c_char,
) -> TypioResult {
    if registry.is_null() || tag.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let tag_str = unsafe { CStr::from_ptr(tag) }
        .to_string_lossy()
        .into_owned();
    let reg = unsafe { &mut *registry };
    activate_language_with_config(reg, &tag_str)
}

fn cycle_language_c(registry: *mut TypioRegistry, direction: SwitchDirection) -> TypioResult {
    if registry.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut *registry };
    let enabled = enabled_languages(reg);
    match reg.inner.cycle_language(&enabled, direction) {
        Some(tag) => activate_language_with_config(reg, &tag),
        None => TypioResult::TypioErrorNotFound,
    }
}

/// Switch to the next language in the enabled cycle.
///
/// Returns `TypioErrorNotFound` when no languages are enabled or declared,
/// so hosts can fall back to engine cycling.
#[no_mangle]
pub extern "C" fn typio_registry_next_language(registry: *mut TypioRegistry) -> TypioResult {
    cycle_language_c(registry, SwitchDirection::Next)
}

/// Switch to the previous language in the enabled cycle.
#[no_mangle]
pub extern "C" fn typio_registry_prev_language(registry: *mut TypioRegistry) -> TypioResult {
    cycle_language_c(registry, SwitchDirection::Previous)
}

/// Activate the persisted last-used language, falling back to the first
/// enabled language. Hosts call this once at startup after engine discovery.
///
/// Returns `TypioErrorNotFound` when no languages are enabled or declared.
#[no_mangle]
pub extern "C" fn typio_registry_restore_language(registry: *mut TypioRegistry) -> TypioResult {
    if registry.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut *registry };
    let enabled = enabled_languages(reg);
    if enabled.is_empty() {
        return TypioResult::TypioErrorNotFound;
    }
    let target = reg
        .inner
        .last_used_language()
        .filter(|t| enabled.iter().any(|e| e.eq_ignore_ascii_case(t)))
        .map(str::to_string)
        .unwrap_or_else(|| enabled[0].clone());
    activate_language_with_config(reg, &target)
}

/* -------------------------------------------------------------------------- */
/* Auto-activation                                                            */
/* -------------------------------------------------------------------------- */

/// Activate the most recently used voice engine, falling back to the first
/// available voice engine if no state exists.
#[no_mangle]
pub extern "C" fn typio_registry_activate_last_used_voice(
    registry: *mut TypioRegistry,
) -> TypioResult {
    if registry.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut *registry };
    match reg.inner.activate_last_used_voice() {
        Ok(()) => {
            unsafe { notify_voice_changed(reg) };
            TypioResult::TypioOk
        }
        Err(crate::core::engine::EngineError::NotFound) => TypioResult::TypioErrorNotFound,
        Err(_) => TypioResult::TypioError,
    }
}

/* -------------------------------------------------------------------------- */
/* Commit notification                                                        */
/* -------------------------------------------------------------------------- */

/// Notify the registry that the active keyboard engine produced a commit.
#[no_mangle]
pub extern "C" fn typio_registry_notify_keyboard_commit(registry: *mut TypioRegistry) {
    if registry.is_null() {
        return;
    }
    let reg = unsafe { &mut (*registry).inner };
    reg.notify_keyboard_commit();
}

/// Notify the registry that the active voice engine produced a commit.
#[no_mangle]
pub extern "C" fn typio_registry_notify_voice_commit(registry: *mut TypioRegistry) {
    if registry.is_null() {
        return;
    }
    let reg = unsafe { &mut (*registry).inner };
    reg.notify_voice_commit();
}

/* -------------------------------------------------------------------------- */
/* Engine command surface (ADR-0008)                                          */
/* -------------------------------------------------------------------------- */

fn map_engine_error(err: crate::core::engine::EngineError) -> TypioResult {
    use crate::core::engine::EngineError::*;
    match err {
        NotFound => TypioResult::TypioErrorNotFound,
        NotSupported => TypioResult::TypioErrorEngineNotAvailable,
        InvalidArgument => TypioResult::TypioErrorInvalidArgument,
        _ => TypioResult::TypioError,
    }
}

/// Invoke a command on the currently active keyboard engine.
///
/// Returns `TypioErrorEngineNotAvailable` if the engine does not support commands.
#[no_mangle]
pub extern "C" fn typio_registry_invoke_active_keyboard_command(
    registry: *mut TypioRegistry,
    id: *const c_char,
) -> TypioResult {
    if registry.is_null() || id.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut (*registry).inner };
    let id_str = unsafe { CStr::from_ptr(id) }.to_string_lossy();
    match reg.invoke_active_keyboard_command(&id_str) {
        Ok(()) => TypioResult::TypioOk,
        Err(e) => map_engine_error(e),
    }
}

/// Invoke a command on a named engine (ADR-0008).
#[no_mangle]
pub extern "C" fn typio_registry_invoke_command(
    registry: *mut TypioRegistry,
    engine_name: *const c_char,
    id: *const c_char,
) -> TypioResult {
    if registry.is_null() || engine_name.is_null() || id.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut (*registry).inner };
    let name = unsafe { CStr::from_ptr(engine_name) }.to_string_lossy();
    let id_str = unsafe { CStr::from_ptr(id) }.to_string_lossy();
    match reg.invoke_command(&name, &id_str) {
        Ok(()) => TypioResult::TypioOk,
        Err(e) => map_engine_error(e),
    }
}

/// List the commands exposed by a named engine (ADR-0008).
///
/// Returns a freshly-allocated array of `TypioEngineCommand`; both the array
/// and its interior strings are owned by the caller and must be released
/// with `typio_engine_command_list_free`. Returns NULL on error or when the
/// engine exposes no commands (sets `*out_count = 0`).
#[no_mangle]
pub extern "C" fn typio_registry_list_commands(
    registry: *mut TypioRegistry,
    engine_name: *const c_char,
    out_count: *mut usize,
) -> *mut TypioEngineCommand {
    if !out_count.is_null() {
        unsafe { *out_count = 0 };
    }
    if registry.is_null() || engine_name.is_null() || out_count.is_null() {
        return ptr::null_mut();
    }
    let reg = unsafe { &mut (*registry).inner };
    let name = unsafe { CStr::from_ptr(engine_name) }.to_string_lossy();
    let commands = match reg.list_commands(&name) {
        Ok(v) => v,
        Err(_) => return ptr::null_mut(),
    };
    if commands.is_empty() {
        return ptr::null_mut();
    }
    let mut boxed: Vec<TypioEngineCommand> = commands
        .into_iter()
        .map(|c| TypioEngineCommand {
            id: CString::new(c.id).unwrap_or_default().into_raw() as *const c_char,
            label: CString::new(c.label).unwrap_or_default().into_raw() as *const c_char,
        })
        .collect();
    unsafe { *out_count = boxed.len() };
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

/// Free a command array returned by `typio_registry_list_commands`.
///
/// Releases the array and every interior string. No-op for NULL.
#[no_mangle]
pub extern "C" fn typio_engine_command_list_free(commands: *mut TypioEngineCommand, count: usize) {
    if commands.is_null() || count == 0 {
        return;
    }
    unsafe {
        let slice = std::slice::from_raw_parts_mut(commands, count);
        for cmd in slice.iter_mut() {
            if !cmd.id.is_null() {
                let _ = CString::from_raw(cmd.id as *mut c_char);
            }
            if !cmd.label.is_null() {
                let _ = CString::from_raw(cmd.label as *mut c_char);
            }
        }
        let _ = Vec::from_raw_parts(commands, count, count);
    }
}

/// Notify a named engine that one of its config keys changed (ADR-0008).
///
/// The host calls this after writing `engines.<name>.<key>` through the
/// unified config tree. No-op if the engine does not implement
/// `on_config_change`.
#[no_mangle]
pub extern "C" fn typio_registry_notify_config_change(
    registry: *mut TypioRegistry,
    engine_name: *const c_char,
    key: *const c_char,
    value: *const c_char,
) -> TypioResult {
    if registry.is_null() || engine_name.is_null() || key.is_null() || value.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let reg = unsafe { &mut (*registry).inner };
    let name = unsafe { CStr::from_ptr(engine_name) }.to_string_lossy();
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    let value_str = unsafe { CStr::from_ptr(value) }.to_string_lossy();
    match reg.notify_config_change(&name, &key_str, &value_str) {
        Ok(()) => TypioResult::TypioOk,
        Err(e) => map_engine_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_check_rejects_null() {
        assert!(!unsafe { typio_engine_abi_check(ptr::null()) });
    }

    #[test]
    fn abi_check_accepts_current_and_older_minor() {
        let exact = TypioAbiVersion {
            major: typio_abi::TYPIO_ENGINE_ABI_MAJOR,
            minor: typio_abi::TYPIO_ENGINE_ABI_MINOR,
        };
        assert!(unsafe { typio_engine_abi_check(&exact) });

        let older_minor = TypioAbiVersion {
            major: typio_abi::TYPIO_ENGINE_ABI_MAJOR,
            minor: 0,
        };
        assert!(unsafe { typio_engine_abi_check(&older_minor) });
    }

    #[test]
    fn abi_check_rejects_major_mismatch_and_newer_minor() {
        let wrong_major = TypioAbiVersion {
            major: typio_abi::TYPIO_ENGINE_ABI_MAJOR + 1,
            minor: typio_abi::TYPIO_ENGINE_ABI_MINOR,
        };
        assert!(!unsafe { typio_engine_abi_check(&wrong_major) });

        let newer_minor = TypioAbiVersion {
            major: typio_abi::TYPIO_ENGINE_ABI_MAJOR,
            minor: typio_abi::TYPIO_ENGINE_ABI_MINOR + 1,
        };
        assert!(!unsafe { typio_engine_abi_check(&newer_minor) });
    }
}
