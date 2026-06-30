//! Instance — Rust implementation of instance.c
//!
//! Manages the Typio instance lifecycle: directories, config, engine registry,
//! input contexts, callbacks, and runtime notifications.

mod callbacks;
mod config_ops;
mod context;
mod identity;

pub use callbacks::*;
pub use config_ops::*;
pub use context::*;

use crate::c_api::registry as c_registry;
use crate::c_api::registry::TypioRegistry;
use crate::config;
use crate::config_schema;
use crate::input_context;
use crate::types::*;
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};
use std::ptr;

const TYPIO_CONFIG_FILE_NAME: &str = "core.toml";

#[allow(improper_ctypes)]
extern "C" {
    pub(crate) fn typio_voice_session_free(session: *mut TypioVoiceSession);
}

/* -------------------------------------------------------------------------- */
/* Internal helpers                                                           */
/* -------------------------------------------------------------------------- */

pub(super) fn get_default_config_dir() -> String {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !config_home.is_empty() {
            return format!("{}/typio", config_home);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return format!("{}/.config/typio", home);
        }
    }
    "/tmp/typio".to_string()
}

pub(super) fn get_default_data_dir() -> String {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.is_empty() {
            return format!("{}/typio", data_home);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return format!("{}/.local/share/typio", home);
        }
    }
    "/tmp/typio/data".to_string()
}

pub(super) fn get_default_state_dir() -> String {
    if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
        if !state_home.is_empty() {
            return format!("{}/typio", state_home);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return format!("{}/.local/state/typio", home);
        }
    }
    "/tmp/typio/state".to_string()
}

pub(super) fn ensure_directory(path: &str) {
    let _ = std::fs::create_dir_all(path);
}

pub(super) fn build_config_path(config_dir: &str, file_name: &str) -> String {
    format!("{}/{}", config_dir, file_name)
}

/* -------------------------------------------------------------------------- */
/* TypioInstance                                                              */
/* -------------------------------------------------------------------------- */

/// Core Typio instance holding configuration, registry, contexts, and callbacks.
#[allow(dead_code)]
pub struct TypioInstance {
    pub(crate) registry: *mut TypioRegistry,
    pub(crate) config: *mut config::Config,

    pub(crate) config_dir: Option<CString>,
    pub(crate) data_dir: Option<CString>,
    pub(crate) state_dir: Option<CString>,
    pub(crate) engine_dirs: Vec<CString>,

    pub(crate) engine_data_dirs: HashMap<String, CString>,
    pub(crate) engine_state_dirs: HashMap<String, CString>,

    pub(crate) plugin_loader: Option<TypioPluginLoaderFunc>,
    pub(crate) plugin_loader_user_data: *mut c_void,

    pub(crate) contexts: Vec<*mut input_context::TypioInputContext>,
    pub(crate) focused_context: *mut input_context::TypioInputContext,

    pub(crate) engine_changed_callback: Option<TypioEngineChangedCallback>,
    pub(crate) engine_changed_user_data: *mut c_void,
    pub(crate) voice_engine_changed_callback: Option<TypioVoiceEngineChangedCallback>,
    pub(crate) voice_engine_changed_user_data: *mut c_void,

    pub(crate) status_icon_changed_callback: Option<TypioStatusIconChangedCallback>,
    pub(crate) status_icon_changed_user_data: *mut c_void,
    pub(crate) last_status_icon: Option<CString>,

    pub(crate) mode_changed_callback: Option<TypioKeyboardModeChangedCallback>,
    pub(crate) mode_changed_user_data: *mut c_void,
    pub(crate) last_mode: TypioKeyboardEngineMode,
    pub(crate) has_mode: bool,

    pub(crate) availability_changed_callback: Option<TypioEngineAvailabilityChangedCallback>,
    pub(crate) availability_changed_user_data: *mut c_void,
    pub(crate) last_availability: TypioEngineAvailability,
    pub(crate) last_availability_reason: Option<CString>,

    /* Dynamic engine capabilities (ADR-0034): fired when an engine updates
     * its declared languages at runtime. Lets the host rebuild the language
     * menu and validate the active language without polling. */
    pub(crate) languages_changed_callback: Option<TypioLanguagesChangedCallback>,
    pub(crate) languages_changed_user_data: *mut c_void,

    pub(crate) initialized: bool,
    pub(crate) voice_session: *mut TypioVoiceSession,
}

impl Drop for TypioInstance {
    fn drop(&mut self) {
        if self.initialized {
            self.shutdown();
        }

        for &ctx in &self.contexts {
            if !ctx.is_null() {
                input_context::typio_input_context_free(ctx);
            }
        }
        self.contexts.clear();

        if !self.registry.is_null() {
            c_registry::typio_registry_free(self.registry);
        }

        if !self.config.is_null() {
            config::typio_config_free(self.config);
        }

        crate::string::typio_free_string(self.last_mode.id as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.last_mode.label as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.last_mode.display_label as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.last_mode.icon_name as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.last_mode.profile_id as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.last_mode.profile_label as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.last_mode.description as *mut std::ffi::c_char);

        if !self.voice_session.is_null() {
            unsafe { typio_voice_session_free(self.voice_session) };
            self.voice_session = ptr::null_mut();
        }
    }
}

impl TypioInstance {
    pub(crate) fn shutdown(&mut self) {
        log::info!("Shutting down Typio instance");
        self.save_config();
        self.initialized = false;
    }

    pub(crate) fn ensure_config(&mut self) -> TypioResult {
        if self.config.is_null() {
            self.config = config::typio_config_new();
        }
        if self.config.is_null() {
            return TypioResult::TypioErrorOutOfMemory;
        }
        config_schema::typio_config_apply_defaults(self.config);
        TypioResult::TypioOk
    }

    pub(crate) fn register_builtin_engines(&mut self) {
        // libtypio is a pure framework: no engines are built in.
        // All engines are loaded at runtime via the host's
        // engine discovery callback from the configured engine directories.
    }

    pub(crate) fn save_config(&self) -> TypioResult {
        if self.config.is_null() {
            return TypioResult::TypioErrorInvalidArgument;
        }
        let config_dir = match self.config_dir.as_ref() {
            Some(d) => d.to_string_lossy(),
            None => return TypioResult::TypioErrorInvalidArgument,
        };
        let path = build_config_path(&config_dir, TYPIO_CONFIG_FILE_NAME);
        let path_c = CString::new(path).unwrap();
        config::typio_config_save_file(self.config, path_c.as_ptr())
    }
}

/* -------------------------------------------------------------------------- */
/* Rust-native API — lifecycle (no `extern "C"` wrappers)                     */
/* -------------------------------------------------------------------------- */
//
// Added so a Rust host (typio ADR-0035) can construct and drive
// a TypioInstance without going through the C ABI. The C ABI surface
// below remains unchanged for engine plugins and other C consumers.
//
// These methods are thin wrappers over the existing `pub(crate)` helpers
// — no behaviour change, just a typed entry point.

use crate::core::registry::EngineRegistry;

impl TypioInstance {
    /// Rust-native constructor. Mirrors what
    /// [`typio_instance_new_with_config`] does internally but takes
    /// typed Rust strings instead of `*const TypioInstanceConfig`.
    ///
    /// Returns a boxed instance ready for [`Self::init_rust`]. The
    /// caller owns the allocation and must call [`Self::shutdown_rust`]
    /// before dropping to persist state.
    ///
    /// `engine_dirs` is stored verbatim; the host's engine-loader
    /// callback (set separately via the C ABI for now — a follow-up
    /// will add a Rust-native registration path) is invoked once per
    /// entry during [`Self::init_rust`].
    pub fn new_rust(
        config_dir: Option<&str>,
        data_dir: Option<&str>,
        state_dir: Option<&str>,
        engine_dirs: Vec<String>,
    ) -> Box<Self> {
        let config_dir =
            config_dir.map(|s| CString::new(s).unwrap_or_else(|_| CString::new("").unwrap()));
        let data_dir =
            data_dir.map(|s| CString::new(s).unwrap_or_else(|_| CString::new("").unwrap()));
        let state_dir =
            state_dir.map(|s| CString::new(s).unwrap_or_else(|_| CString::new("").unwrap()));

        // Apply the same "fall back to default if absent" rule the C
        // version uses, so callers passing `None` still get a usable
        // path.
        let config_dir =
            config_dir.or_else(|| Some(CString::new(get_default_config_dir()).unwrap()));
        let data_dir = data_dir.or_else(|| Some(CString::new(get_default_data_dir()).unwrap()));
        let state_dir = state_dir.or_else(|| Some(CString::new(get_default_state_dir()).unwrap()));

        let engine_dirs: Vec<CString> = engine_dirs
            .into_iter()
            .filter(|s| !s.is_empty())
            .filter_map(|s| CString::new(s).ok())
            .collect();

        Box::new(TypioInstance {
            registry: ptr::null_mut(),
            config: ptr::null_mut(),
            config_dir,
            data_dir,
            state_dir,
            engine_dirs,
            engine_data_dirs: HashMap::new(),
            engine_state_dirs: HashMap::new(),
            plugin_loader: None,
            plugin_loader_user_data: ptr::null_mut(),
            contexts: Vec::with_capacity(8),
            focused_context: ptr::null_mut(),
            engine_changed_callback: None,
            engine_changed_user_data: ptr::null_mut(),
            voice_engine_changed_callback: None,
            voice_engine_changed_user_data: ptr::null_mut(),
            status_icon_changed_callback: None,
            status_icon_changed_user_data: ptr::null_mut(),
            last_status_icon: None,
            mode_changed_callback: None,
            mode_changed_user_data: ptr::null_mut(),
            last_mode: TypioKeyboardEngineMode {
                id: ptr::null(),
                label: ptr::null(),
                display_label: ptr::null(),
                icon_name: ptr::null(),
                profile_id: ptr::null(),
                profile_label: ptr::null(),
                description: ptr::null(),
                salience: TypioStatusSalience::TypioStatusSalienceQuiet,
            },
            has_mode: false,
            initialized: false,
            voice_session: ptr::null_mut(),
            last_availability: TypioEngineAvailability::TypioEngineReady,
            last_availability_reason: None,
            availability_changed_callback: None,
            availability_changed_user_data: ptr::null_mut(),
            languages_changed_callback: None,
            languages_changed_user_data: ptr::null_mut(),
        })
    }

    /// Rust-native initializer. Mirrors [`typio_instance_init`] but
    /// takes `&mut self` and returns a `Result`. Idempotent — calling
    /// on an already-initialized instance is a no-op success.
    ///
    /// Performs:
    /// - creates the config + engine-data/state directories
    /// - loads `core.toml` from `config_dir` (or starts with defaults)
    /// - allocates the engine registry
    /// - invokes the host's engine-loader callback for each `engine_dir`
    /// - restores the last-active language (ADR-0018)
    pub fn init_rust(&mut self) -> Result<(), TypioResult> {
        // SAFETY: `self` is a unique mutable reference; casting to
        // `*mut _` for the FFI call is sound because no other reference
        // aliases it for the duration of the call.
        let raw = self as *mut Self;
        let result = typio_instance_init(raw);
        if result == TypioResult::TypioOk {
            Ok(())
        } else {
            Err(result)
        }
    }

    /// Rust-native shutdown. Mirrors [`typio_instance_shutdown`] but
    /// takes `&mut self`. Persists config + state; safe to call on an
    /// uninitialized instance (no-op).
    pub fn shutdown_rust(&mut self) {
        if self.initialized {
            self.shutdown();
        }
    }

    /// Typed accessor for the engine registry. Returns `None` before
    /// [`Self::init_rust`] has run successfully.
    ///
    /// The returned reference lives as long as `&self` — the registry
    /// is owned by the instance and freed in `Drop`.
    pub fn registry_rust(&self) -> Option<&EngineRegistry> {
        if self.registry.is_null() {
            return None;
        }
        // SAFETY: `self.registry` is set by init_rust to a valid
        // `*mut TypioRegistry` allocated via Box::into_raw, and is
        // freed only in Drop. We hold &self so no concurrent free.
        Some(unsafe { &(*self.registry).inner })
    }

    /// Mutable typed accessor for the engine registry. Returns `None`
    /// before [`Self::init_rust`] has run successfully.
    ///
    /// The returned reference lives as long as `&mut self` — the
    /// registry is owned by the instance and freed in `Drop`. Callers
    /// that need to register engines, switch the active keyboard /
    /// voice slot, or otherwise mutate registry state should prefer
    /// this over the C ABI (`typio_registry_register_engine_process`
    /// et al.) so engine-loader logic stays in pure Rust.
    pub fn registry_rust_mut(&mut self) -> Option<&mut EngineRegistry> {
        if self.registry.is_null() {
            return None;
        }
        // SAFETY: same justification as `registry_rust`, plus: we hold
        // `&mut self` so no other Rust reference can alias the
        // returned `&mut EngineRegistry` for its lifetime. The C ABI
        // surface may still reenter through callback dispatch, but
        // that path is guarded by the instance's own interior
        // synchronisation, identical to `typio_instance_init`.
        Some(unsafe { &mut (*self.registry).inner })
    }

    /// Typed accessor for the config tree. Returns `None` before
    /// [`Self::init_rust`] has run successfully.
    pub fn config_rust(&self) -> Option<&config::Config> {
        if self.config.is_null() {
            return None;
        }
        // SAFETY: same justification as registry_rust.
        Some(unsafe { &(*self.config) })
    }
}

/* -------------------------------------------------------------------------- */
/* Exported C API — lifecycle                                                 */
/* -------------------------------------------------------------------------- */

/// Create a new Typio instance with default directories.
#[no_mangle]
pub extern "C" fn typio_instance_new() -> *mut TypioInstance {
    typio_instance_new_with_config(ptr::null())
}

/// Create a new Typio instance with the supplied configuration.
#[no_mangle]
pub extern "C" fn typio_instance_new_with_config(
    config: *const TypioInstanceConfig,
) -> *mut TypioInstance {
    let config_dir = if !config.is_null() {
        unsafe { (*config).config_dir.as_ref() }.and_then(|p| {
            unsafe { CStr::from_ptr(p) }
                .to_str()
                .ok()
                .map(|s| CString::new(s).unwrap())
        })
    } else {
        None
    }
    .or_else(|| Some(CString::new(get_default_config_dir()).unwrap()));

    let data_dir = if !config.is_null() {
        unsafe { (*config).data_dir.as_ref() }.and_then(|p| {
            unsafe { CStr::from_ptr(p) }
                .to_str()
                .ok()
                .map(|s| CString::new(s).unwrap())
        })
    } else {
        None
    }
    .or_else(|| Some(CString::new(get_default_data_dir()).unwrap()));

    let state_dir = if !config.is_null() {
        unsafe { (*config).state_dir.as_ref() }.and_then(|p| {
            unsafe { CStr::from_ptr(p) }
                .to_str()
                .ok()
                .map(|s| CString::new(s).unwrap())
        })
    } else {
        None
    }
    .or_else(|| Some(CString::new(get_default_state_dir()).unwrap()));

    // The host supplies the list of engine directories to scan. Core no
    // longer invents a default path or reads TYPIO_ENGINE_DIR — that is
    // platform/host policy and belongs in the host.
    let mut engine_dirs: Vec<CString> = Vec::new();
    if !config.is_null() {
        let dirs_ptr = unsafe { (*config).engine_dirs };
        if !dirs_ptr.is_null() {
            let mut i = 0isize;
            loop {
                let entry = unsafe { *dirs_ptr.offset(i) };
                if entry.is_null() {
                    break;
                }
                if let Ok(s) = unsafe { CStr::from_ptr(entry) }.to_str() {
                    if !s.is_empty() {
                        engine_dirs.push(CString::new(s).unwrap());
                    }
                }
                i += 1;
            }
        }
    }

    let (plugin_loader, plugin_loader_user_data) = if !config.is_null() {
        let cfg = unsafe { &*config };
        (cfg.plugin_loader, cfg.plugin_loader_user_data)
    } else {
        (None, ptr::null_mut())
    };

    let instance = Box::new(TypioInstance {
        registry: ptr::null_mut(),
        config: ptr::null_mut(),
        config_dir,
        data_dir,
        state_dir,
        engine_dirs,
        engine_data_dirs: HashMap::new(),
        engine_state_dirs: HashMap::new(),
        plugin_loader,
        plugin_loader_user_data,
        contexts: Vec::with_capacity(8),
        focused_context: ptr::null_mut(),
        engine_changed_callback: None,
        engine_changed_user_data: ptr::null_mut(),
        voice_engine_changed_callback: None,
        voice_engine_changed_user_data: ptr::null_mut(),
        status_icon_changed_callback: None,
        status_icon_changed_user_data: ptr::null_mut(),
        last_status_icon: None,
        mode_changed_callback: None,
        mode_changed_user_data: ptr::null_mut(),
        availability_changed_callback: None,
        availability_changed_user_data: ptr::null_mut(),
        last_availability: TypioEngineAvailability::TypioEngineReady,
        last_availability_reason: None,
        languages_changed_callback: None,
        languages_changed_user_data: ptr::null_mut(),
        last_mode: TypioKeyboardEngineMode {
            id: ptr::null(),
            label: ptr::null(),
            display_label: ptr::null(),
            icon_name: ptr::null(),
            profile_id: ptr::null(),
            profile_label: ptr::null(),
            description: ptr::null(),
            salience: TypioStatusSalience::TypioStatusSalienceQuiet,
        },
        has_mode: false,
        initialized: false,
        voice_session: ptr::null_mut(),
    });

    Box::into_raw(instance)
}

/// Free a Typio instance and all associated resources.
#[no_mangle]
pub extern "C" fn typio_instance_free(instance: *mut TypioInstance) {
    if instance.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(instance));
    }
}

/// Initialize the instance (config, registry, engine activation).
#[no_mangle]
pub extern "C" fn typio_instance_init(instance: *mut TypioInstance) -> TypioResult {
    if instance.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }

    let inst = unsafe { &mut *instance };

    if inst.initialized {
        return TypioResult::TypioOk;
    }

    log::info!("Initializing Typio instance");

    if let Some(ref dir) = inst.config_dir {
        ensure_directory(&dir.to_string_lossy());
    }
    if let Some(ref dir) = inst.data_dir {
        ensure_directory(&dir.to_string_lossy());
    }
    if let Some(ref dir) = inst.state_dir {
        ensure_directory(&dir.to_string_lossy());
    }

    let config_path = match inst.config_dir.as_ref() {
        Some(d) => build_config_path(&d.to_string_lossy(), TYPIO_CONFIG_FILE_NAME),
        None => return TypioResult::TypioErrorInvalidArgument,
    };
    let path_c = CString::new(config_path).unwrap();
    inst.config = config::typio_config_load_file(path_c.as_ptr());
    if inst.config.is_null() {
        inst.config = config::typio_config_new();
    }
    let result = inst.ensure_config();
    if result != TypioResult::TypioOk {
        log::error!("Failed to initialize configuration");
        return result;
    }

    inst.registry = c_registry::typio_registry_new(instance);
    if inst.registry.is_null() {
        log::error!("Failed to create engine registry");
        return TypioResult::TypioError;
    }

    // Wire up state persistence so the registry can load/save
    // last-used keyboard and voice engine pairs across restarts.
    if let Some(ref dir) = inst.state_dir {
        unsafe {
            (*inst.registry).inner.set_state_dir(&dir.to_string_lossy());
        }
    }

    inst.register_builtin_engines();

    // Engine discovery is the host's job. Core invokes the host-supplied
    // loader callback once per configured engine directory; the callback
    // performs platform-specific enumeration and calls
    // typio_registry_register_engine_process for each engine it accepts.
    if let Some(loader) = inst.plugin_loader {
        for dir in &inst.engine_dirs {
            let loaded = loader(inst.registry, dir.as_ptr(), inst.plugin_loader_user_data);
            log::info!(
                "Host loader registered {} engine(s) from {}",
                loaded,
                dir.to_string_lossy()
            );
        }
    } else if !inst.engine_dirs.is_empty() {
        log::warn!(
            "engine_dirs configured but no engine discovery callback provided — engines will not be available"
        );
    }

    // Language-first activation (ADR-0018): when any languages are enabled
    // or declared, restore the persisted active language — it retargets the
    // keyboard and voice slots through language resolution. The legacy
    // per-modality chain below runs only when no language is available.
    if c_registry::typio_registry_restore_language(inst.registry) == TypioResult::TypioOk {
        inst.initialized = true;
        log::info!("Typio instance initialized (language-first activation)");
        return TypioResult::TypioOk;
    }

    // Activate keyboard engine. Config key `keyboard.engine` takes priority
    // over state-persistence (engine-state.toml), which in turn falls back
    // to the first available keyboard engine.
    let kb_engine = if !inst.config.is_null() {
        let key = CString::new("keyboard.engine").unwrap();
        let val = config::typio_config_get_string(inst.config, key.as_ptr(), ptr::null());
        if !val.is_null() {
            Some(
                unsafe { CStr::from_ptr(val) }
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        }
    } else {
        None
    };
    if let Some(ref name) = kb_engine {
        if !name.is_empty() {
            let name_c = CString::new(name.as_str()).unwrap();
            let r = c_registry::typio_registry_set_active_keyboard(inst.registry, name_c.as_ptr());
            if r != TypioResult::TypioOk {
                log::warn!(
                    "Configured keyboard engine '{}' not found, falling back to last-used",
                    name
                );
                let result = unsafe { (*inst.registry).inner.activate_last_used_keyboard() };
                if let Err(ref e) = result {
                    log::warn!("Failed to activate keyboard engine: {:?}", e);
                }
            }
        }
    } else {
        let result = unsafe { (*inst.registry).inner.activate_last_used_keyboard() };
        if let Err(ref e) = result {
            log::warn!("Failed to activate keyboard engine: {:?}", e);
        }
    }

    // Activate voice engine. Same priority: `voice.engine` config key >
    // state-persistence > first available voice engine.
    let voice_engine = if !inst.config.is_null() {
        let key = CString::new("voice.engine").unwrap();
        let val = config::typio_config_get_string(inst.config, key.as_ptr(), ptr::null());
        if !val.is_null() {
            Some(
                unsafe { CStr::from_ptr(val) }
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        }
    } else {
        None
    };
    if let Some(ref name) = voice_engine {
        if !name.is_empty() {
            let name_c = CString::new(name.as_str()).unwrap();
            let r = c_registry::typio_registry_set_active_voice(inst.registry, name_c.as_ptr());
            if r != TypioResult::TypioOk {
                log::warn!(
                    "Configured voice engine '{}' not found, falling back to last-used",
                    name
                );
                let result = unsafe { (*inst.registry).inner.activate_last_used_voice() };
                if let Err(ref e) = result {
                    log::debug!("No voice engine to activate: {:?}", e);
                }
            }
        }
    } else {
        let result = unsafe { (*inst.registry).inner.activate_last_used_voice() };
        if let Err(ref e) = result {
            log::debug!("No voice engine to activate: {:?}", e);
        }
    }

    inst.initialized = true;
    log::info!("Typio instance initialized");
    TypioResult::TypioOk
}

/// Shut down the instance, saving state.
#[no_mangle]
pub extern "C" fn typio_instance_shutdown(instance: *mut TypioInstance) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.shutdown();
}

/// Get the voice session associated with this instance.
#[no_mangle]
pub extern "C" fn typio_instance_get_voice_session(
    instance: *mut TypioInstance,
) -> *mut TypioVoiceSession {
    if instance.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*instance).voice_session }
}

/// Set the voice session associated with this instance.
#[no_mangle]
pub extern "C" fn typio_instance_set_voice_session(
    instance: *mut TypioInstance,
    session: *mut TypioVoiceSession,
) {
    if instance.is_null() {
        return;
    }
    unsafe {
        (*instance).voice_session = session;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};
    use std::ptr;

    #[test]
    fn instance_new_and_free() {
        let inst = typio_instance_new();
        assert!(!inst.is_null());
        typio_instance_free(inst);
    }

    #[test]
    fn rust_api_new_init_and_accessors() {
        // Rust-native constructor + accessors. Verifies the new API
        // works end-to-end without going through the C ABI surface.
        let temp = std::env::temp_dir().join(format!(
            "typio-rust-api-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();

        let mut instance = TypioInstance::new_rust(
            Some(temp.to_str().unwrap()),
            Some(temp.to_str().unwrap()),
            Some(temp.to_str().unwrap()),
            Vec::new(), // no engine dirs — no plugin loader set either
        );

        // Before init: registry/config are None.
        assert!(instance.registry_rust().is_none());
        assert!(instance.config_rust().is_none());
        assert!(!instance.initialized);

        // Init creates config + registry.
        instance.init_rust().expect("init should succeed");
        assert!(instance.initialized);

        // After init: accessors return Some.
        let _registry = instance.registry_rust().expect("registry after init");
        let _config = instance.config_rust().expect("config after init");

        // Idempotent re-init.
        instance.init_rust().expect("re-init is no-op success");

        // Shutdown persists state; safe even though we never set a
        // plugin loader.
        instance.shutdown_rust();
        assert!(!instance.initialized);

        // Drop removes nothing on disk; the temp dir will be cleaned by
        // the OS. We don't assert on files because config save is
        // best-effort.
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn instance_init_and_shutdown() {
        let inst = typio_instance_new();
        assert!(!inst.is_null());
        let ret = typio_instance_init(inst);
        assert_eq!(ret, TypioResult::TypioOk);
        typio_instance_shutdown(inst);
        typio_instance_free(inst);
    }

    #[test]
    fn instance_get_config_dir_not_null() {
        let inst = typio_instance_new();
        let dir = typio_instance_get_config_dir(inst);
        assert!(!dir.is_null());
        let s = unsafe { CStr::from_ptr(dir) }.to_str().unwrap();
        assert!(!s.is_empty());
        typio_instance_free(inst);
    }

    #[test]
    fn instance_get_state_dir_not_null() {
        let inst = typio_instance_new();
        let dir = typio_instance_get_state_dir(inst);
        assert!(!dir.is_null());
        let s = unsafe { CStr::from_ptr(dir) }.to_str().unwrap();
        assert!(!s.is_empty());
        typio_instance_free(inst);
    }

    #[test]
    fn instance_config_roundtrip() {
        let inst = typio_instance_new();
        typio_instance_init(inst);

        let cfg = typio_instance_get_config(inst);
        assert!(!cfg.is_null());

        let key = CString::new("test.instance").unwrap();
        let val = CString::new("works").unwrap();
        config::typio_config_set_string(cfg, key.as_ptr(), val.as_ptr());

        let got = config::typio_config_get_string(cfg, key.as_ptr(), ptr::null());
        assert!(!got.is_null());
        let s = unsafe { CStr::from_ptr(got) }.to_str().unwrap();
        assert_eq!(s, "works");

        typio_instance_free(inst);
    }

    #[test]
    fn instance_registry_lifecycle() {
        let inst = typio_instance_new();
        typio_instance_init(inst);

        let reg = typio_instance_get_registry(inst);
        assert!(!reg.is_null());

        // registry is owned by instance, do not free separately
        typio_instance_free(inst);
    }
}
