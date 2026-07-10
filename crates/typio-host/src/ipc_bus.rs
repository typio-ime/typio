//! UDS request bus — wires [`UdsServer`] to [`StatusService`] and broadcasts
//! state changes to subscribed clients.
//!
//! Port of `src/ipc/ipc_bus.c`. The C module couples UDS framing, request
//! dispatch, and libtypio state mutation in one file. This Rust port keeps the
//! already-ported [`UdsServer`] and [`StatusService`] separate and only adds
//! the thin gluing layer plus a libtypio-backed [`ServiceBackend`] impl.
//!
//! ## Responsibilities
//!
//! - Install a request handler on the [`UdsServer`] that parses JSON-RPC,
//!   dispatches through [`StatusService`], and forwards any subscription change
//!   back to the server.
//! - Provide [`IpcBus::emit`] so a [`StateController`](crate::state_controller)
//!   listener can push notifications to subscribed UDS clients.
//! - Implement [`ServiceBackend`] for a raw [`TypioInstance`] pointer so the
//!   generic dispatch service can drive the live framework state.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::ptr;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::ipc::framing::{Id, Request, Response, StandardError};
use crate::service::{
    ConfigEntry, ConfigField, ConfigGetOutcome, ConfigSource, CycleLanguageOutcome, EngineCommand,
    EngineKind, FieldType, InvokeOutcome, RuntimeState, ServiceBackend, SvcError,
};
use crate::state_controller::RegistryView;
use crate::uds_server::{ClientId, RequestOutcome, SubscriptionUpdate, UdsServer};

/// The live UDS + dispatch surface.
pub struct IpcBus {
    server: UdsServer,
    service: Rc<RefCell<crate::service::StatusService<TypioBackend>>>,
}

impl IpcBus {
    /// Wrap a bound UDS server and a configured service. The constructor installs
    /// the JSON-RPC request handler on the server.
    pub fn new(server: UdsServer, service: crate::service::StatusService<TypioBackend>) -> Self {
        let svc_rc = Rc::new(RefCell::new(service));

        let pending_sub: Arc<Mutex<Option<SubscriptionUpdate>>> = Arc::new(Mutex::new(None));

        let mut server = server;
        server.set_handler({
            let pending_sub = pending_sub.clone();
            let svc_clone = svc_rc.clone();
            move |json: &str, client_id: ClientId| {
                let mut svc = svc_clone.borrow_mut();

                let (req, id) = match parse_tip_request(json) {
                    Ok(request) => request,
                    Err(error) => {
                        let resp = Response::error(Id::Null, error.code(), error.message());
                        return RequestOutcome::respond(resp.to_json().unwrap_or_default());
                    }
                };
                let params = req.params.as_ref().cloned().unwrap_or(Value::Null);

                // Capture the subscription request (if any) so it can be applied
                // by the server after this closure returns.
                let pending = pending_sub.clone();
                svc.set_subscribe_callback({
                    let p = pending.clone();
                    move |_token, topics| {
                        let update = if topics.is_empty() {
                            SubscriptionUpdate::Wildcard
                        } else {
                            SubscriptionUpdate::Topics(topics)
                        };
                        *p.lock().unwrap() = Some(update);
                    }
                });

                let resp = svc.handle(&req.method, &params, id, client_id.0);
                let sub = pending.lock().unwrap().take();

                let json_resp = match resp.to_json() {
                    Ok(s) => s,
                    Err(_) => return RequestOutcome::silent(),
                };

                match sub {
                    Some(update) => RequestOutcome::respond_and_subscribe(json_resp, update),
                    None => RequestOutcome::respond(json_resp),
                }
            }
        });

        Self {
            server,
            service: svc_rc,
        }
    }

    /// Install the callback triggered by `daemon.stop`.
    pub fn set_stop_callback<F: FnMut() + 'static>(&mut self, cb: F) {
        self.service.borrow_mut().set_stop_callback(cb);
    }

    /// Install the callback triggered after any IPC-driven state mutation
    /// (engine/language switch, config reload, engine load/unload). The daemon
    /// core uses this to push a `StateRefresh` so derived surfaces (controller
    /// snapshot, tray icon, tooltip) re-sync against the mutated registry.
    pub fn set_state_change_callback<F: FnMut() + 'static>(&mut self, cb: F) {
        self.service.borrow_mut().set_state_change_callback(cb);
    }

    /// Drain pending UDS events. Call once per loop iteration.
    pub fn dispatch(&mut self) {
        self.server.dispatch();
    }

    /// Emit a JSON-RPC notification to every subscribed client matching `topic`.
    pub fn emit(&mut self, topic: &str, payload: &Value) {
        self.server.emit(topic, payload);
    }

    /// The socket path the underlying server is bound to.
    pub fn socket_path(&self) -> std::path::PathBuf {
        self.server.socket_path().to_path_buf()
    }

    /// The epoll fd of the underlying server, suitable for polling.
    pub fn epoll_fd(&self) -> RawFd {
        self.server.epoll_fd()
    }
}

// ── libtypio-backed ServiceBackend ───────────────────────────────────────────

/// A [`ServiceBackend`] that drives a live [`TypioInstance`] through its public
/// C ABI (and the small Rust-native registry accessor where available).
pub struct TypioBackend {
    instance: *mut typio::TypioInstance,
}

impl TypioBackend {
    /// The instance pointer must remain valid for the lifetime of the backend.
    pub fn new(instance: *mut typio::TypioInstance) -> Self {
        Self { instance }
    }

    fn instance(&self) -> Option<&typio::TypioInstance> {
        unsafe { self.instance.as_ref() }
    }

    fn instance_mut(&mut self) -> Option<&mut typio::TypioInstance> {
        unsafe { self.instance.as_mut() }
    }

    fn registry_ptr(&self) -> *mut typio::c_api::registry::TypioRegistry {
        if self.instance.is_null() {
            return ptr::null_mut();
        }
        typio::instance::typio_instance_get_registry(self.instance)
    }

    fn config_ptr(&self) -> *mut typio::config::Config {
        if self.instance.is_null() {
            return ptr::null_mut();
        }
        typio::instance::typio_instance_get_config(self.instance)
    }

    fn registry(&self) -> Option<&typio::core::registry::EngineRegistry> {
        self.instance().and_then(|i| i.registry_rust())
    }
}

impl ServiceBackend for TypioBackend {
    // ── config ──

    fn config_get(&self, key: &str) -> Option<ConfigGetOutcome> {
        let cfg = unsafe { self.config_ptr().as_ref() }?;
        let schema = schema_field(key);
        let current = cfg.value(key);
        if schema.is_none() && current.is_none() {
            return Some(ConfigGetOutcome::Unknown);
        }
        let (value, field_type) = match current {
            Some(value) => (config_value_to_json(value), config_value_type(value)?),
            None => schema_default(schema.as_ref()?)?,
        };
        Some(ConfigGetOutcome::Found {
            value,
            field_type,
            source: if cfg.is_user_value(key) {
                ConfigSource::User
            } else {
                ConfigSource::Default
            },
        })
    }

    fn config_set(&mut self, key: &str, value_str: &str) -> Option<Result<(), SvcError>> {
        let cfg = self.config_ptr();
        if cfg.is_null() {
            return None;
        }
        let schema = schema_field(key);
        let current_type = unsafe { cfg.as_ref() }
            .and_then(|cfg| cfg.value(key))
            .and_then(config_value_type);
        let field_type = match (schema.as_ref().map(|field| field.field_type), current_type) {
            // Preserve list-valued user configuration even though old static
            // schemas represented `languages.enabled` as a string.
            (_, Some(FieldType::Array)) => FieldType::Array,
            (Some(field_type), _) => field_type,
            (None, Some(field_type)) => field_type,
            (None, None) => return Some(Err(SvcError)),
        };
        Some(
            set_config_value(cfg, key, value_str, field_type, schema.as_ref())
                .map_err(|_| SvcError),
        )
    }

    fn config_unset(&mut self, key: &str) -> Option<Result<(), SvcError>> {
        let cfg = self.config_ptr();
        if cfg.is_null() {
            return None;
        }
        let Ok(key_c) = CString::new(key) else {
            return Some(Err(SvcError));
        };
        let has_schema = schema_field(key).is_some();
        match typio::config::typio_config_remove(cfg, key_c.as_ptr()) {
            typio::TypioResult::TypioOk => {
                typio::config_schema::typio_config_apply_defaults(cfg);
                Some(Ok(()))
            }
            typio::TypioResult::TypioErrorNotFound if has_schema => Some(Ok(())),
            typio::TypioResult::TypioErrorNotFound => Some(Err(SvcError)),
            _ => Some(Err(SvcError)),
        }
    }

    fn config_list(&self, prefix: &str) -> Option<Vec<ConfigEntry>> {
        let cfg = unsafe { self.config_ptr().as_ref() }?;
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        let mut field_count = 0usize;
        let fields = typio::config_schema::typio_config_schema_fields(&mut field_count);
        for i in 0..field_count {
            let Some(raw) = (unsafe { fields.add(i).as_ref() }) else {
                continue;
            };
            let Some(field) = config_field_from_raw(raw) else {
                continue;
            };
            let key = field.key.clone();
            if !prefix.is_empty() && !key.starts_with(prefix) {
                continue;
            }
            let (value, field_type) = if let Some(value) = cfg.value(&key) {
                (
                    config_value_to_json(value),
                    config_value_type(value).unwrap_or(field.field_type),
                )
            } else {
                schema_default(&SchemaField::from_raw(raw)?)?
            };
            let mut field = field;
            field.field_type = field_type;
            entries.push(ConfigEntry {
                field,
                value,
                source: if cfg.is_user_value(&key) {
                    ConfigSource::User
                } else {
                    ConfigSource::Default
                },
            });
            seen.insert(key);
        }
        for (key, value) in cfg.iter() {
            if seen.contains(key) || (!prefix.is_empty() && !key.starts_with(prefix)) {
                continue;
            }
            let Some(field_type) = config_value_type(value) else {
                continue;
            };
            entries.push(ConfigEntry {
                field: ConfigField {
                    key: key.to_string(),
                    field_type,
                    label: None,
                    section: key.rsplit_once('.').map(|(section, _)| section.to_string()),
                    choices: None,
                },
                value: config_value_to_json(value),
                source: ConfigSource::User,
            });
        }
        Some(entries)
    }

    fn config_show_text(&self) -> String {
        if self.instance.is_null() {
            return String::new();
        }
        let text = typio::instance::typio_instance_get_config_text(self.instance);
        if text.is_null() {
            return String::new();
        }
        let s = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        typio::string::typio_free_string(text);
        s
    }

    fn config_reload(&mut self) -> Result<(), SvcError> {
        if self.instance.is_null() {
            return Err(SvcError);
        }
        match typio::instance::typio_instance_reload_config(self.instance) {
            typio::TypioResult::TypioOk => Ok(()),
            _ => Err(SvcError),
        }
    }

    fn save_config(&mut self) -> Result<(), SvcError> {
        if self.instance.is_null() {
            return Err(SvcError);
        }
        match typio::instance::typio_instance_save_config(self.instance) {
            typio::TypioResult::TypioOk => Ok(()),
            _ => Err(SvcError),
        }
    }

    fn notify_engine_config(&mut self, engine: &str, key: &str, value: &str) {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return;
        }
        let (Ok(engine_c), Ok(key_c), Ok(value_c)) =
            (c_str(engine), c_str(key), c_str(value))
        else {
            return;
        };
        typio::c_api::registry::typio_registry_notify_config_change(
            reg,
            engine_c.as_ptr(),
            key_c.as_ptr(),
            value_c.as_ptr(),
        );
        if self.engine_info(engine) == Some(EngineKind::Voice) {
            let session = typio::instance::typio_instance_get_voice_session(self.instance);
            if !session.is_null() {
                typio::voice::session::typio_voice_session_reload_engine(session);
            }
        }
    }

    // ── registry ──

    fn registry_present(&self) -> bool {
        self.registry().is_some()
    }

    fn list_keyboards(&self) -> Vec<String> {
        self.registry()
            .map(|r| r.list_keyboards().into_iter().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn list_voices(&self) -> Vec<String> {
        self.registry()
            .map(|r| r.list_voices().into_iter().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn list_languages(&self) -> Vec<String> {
        self.registry()
            .map(|r| r.known_languages())
            .unwrap_or_default()
    }

    fn engine_info(&self, name: &str) -> Option<EngineKind> {
        self.registry()
            .and_then(|r| r.engine_info(name))
            .map(|info| match info.engine_type {
                typio::core::engine::EngineType::Keyboard => EngineKind::Keyboard,
                typio::core::engine::EngineType::Voice => EngineKind::Voice,
            })
    }

    fn engine_display_name(&self, name: &str) -> Option<String> {
        self.registry()
            .and_then(|r| r.engine_info(name))
            .map(|info| info.display_name.clone())
    }

    fn active_keyboard(&self) -> Option<String> {
        self.registry()
            .and_then(|r| r.active_keyboard_name())
            .map(str::to_string)
    }

    fn active_voice(&self) -> Option<String> {
        self.registry()
            .and_then(|r| r.active_voice_name())
            .map(str::to_string)
    }

    fn active_language(&self) -> Option<String> {
        self.registry()
            .and_then(|r| r.active_language())
            .map(str::to_string)
    }

    fn set_active_keyboard(&mut self, name: &str) -> Result<(), SvcError> {
        self.set_active_engine(name, false)
    }

    fn set_active_voice(&mut self, name: &str) -> Result<(), SvcError> {
        self.set_active_engine(name, true)
    }

    fn set_active_language(&mut self, tag: &str) -> Result<(), SvcError> {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return Err(SvcError);
        }
        let tag_c = c_str(tag)?;
        match typio::c_api::registry::typio_registry_set_active_language(reg, tag_c.as_ptr()) {
            typio::TypioResult::TypioOk => Ok(()),
            _ => Err(SvcError),
        }
    }

    fn cycle_keyboard(&mut self, forward: bool) -> Result<(), SvcError> {
        self.cycle_engine(forward, false)
    }

    fn cycle_voice(&mut self, forward: bool) -> Result<(), SvcError> {
        self.cycle_engine(forward, true)
    }

    fn cycle_language(&mut self, forward: bool) -> CycleLanguageOutcome {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return CycleLanguageOutcome::Failed;
        }
        let result = if forward {
            typio::c_api::registry::typio_registry_next_language(reg)
        } else {
            typio::c_api::registry::typio_registry_prev_language(reg)
        };
        match result {
            typio::TypioResult::TypioOk => CycleLanguageOutcome::Ok(self.active_language()),
            typio::TypioResult::TypioErrorNotFound => CycleLanguageOutcome::NoLanguages,
            _ => CycleLanguageOutcome::Failed,
        }
    }

    fn list_commands(&self, name: &str) -> Vec<EngineCommand> {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return Vec::new();
        }
        let Ok(name_c) = c_str(name) else {
            return Vec::new();
        };
        let mut count: usize = 0;
        let commands =
            typio::c_api::registry::typio_registry_list_commands(reg, name_c.as_ptr(), &mut count);
        if commands.is_null() || count == 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let cmd = unsafe { &*commands.add(i) };
            let id = if cmd.id.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(cmd.id) }
                    .to_string_lossy()
                    .into_owned()
            };
            let label = if cmd.label.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(cmd.label) }
                    .to_string_lossy()
                    .into_owned()
            };
            out.push(EngineCommand { id, label });
        }
        typio::c_api::registry::typio_engine_command_list_free(commands, count);
        out
    }

    fn invoke_command(&mut self, name: &str, cmd: &str) -> InvokeOutcome {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return InvokeOutcome::Failed;
        }
        let (Ok(name_c), Ok(cmd_c)) = (c_str(name), c_str(cmd)) else {
            return InvokeOutcome::Failed;
        };
        match typio::c_api::registry::typio_registry_invoke_command(
            reg,
            name_c.as_ptr(),
            cmd_c.as_ptr(),
        ) {
            typio::TypioResult::TypioOk => InvokeOutcome::Ok,
            typio::TypioResult::TypioErrorNotFound => InvokeOutcome::NotFound,
            typio::TypioResult::TypioErrorEngineNotAvailable => InvokeOutcome::NotSupported,
            _ => InvokeOutcome::Failed,
        }
    }

    // ── engine loader ──
    //
    fn engine_load(&mut self, path: &str) -> Result<(), SvcError> {
        let path = canonical_manifest_path(Path::new(path), true)?;
        let registry = self
            .instance_mut()
            .and_then(typio::TypioInstance::registry_rust_mut)
            .ok_or(SvcError)?;
        crate::engine_loader::EngineLoader::with_voice()
            .load_single(registry, &path)
            .map(|_| ())
            .map_err(|_| SvcError)
    }

    fn engine_unload(&mut self, name: &str) -> Result<(), SvcError> {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return Err(SvcError);
        }
        let name_c = c_str(name)?;
        match typio::c_api::registry::typio_registry_unload(reg, name_c.as_ptr()) {
            typio::TypioResult::TypioOk => Ok(()),
            typio::TypioResult::TypioErrorNotFound => Err(SvcError),
            _ => Err(SvcError),
        }
    }

    fn engine_reload(&mut self, name: &str, path: Option<&str>) -> Result<(), SvcError> {
        let instance = self.instance_mut().ok_or(SvcError)?;
        let engine_dirs: Vec<PathBuf> = instance.engine_dirs_rust().map(PathBuf::from).collect();
        let path = match path {
            Some(path) => canonical_manifest_path(Path::new(path), true)?,
            None => {
                let path = crate::engine_loader::find_manifest_for(
                    engine_dirs.iter().map(PathBuf::as_path),
                    name,
                )
                .ok_or(SvcError)?;
                canonical_manifest_path(&path, false)?
            }
        };

        // Fully parse and negotiate before touching the live registry. Once
        // this succeeds, registration after the old entry is removed cannot
        // fail under the single-threaded host unless registry invariants are
        // broken.
        let mut validator = crate::engine_loader::EngineLoader::with_voice();
        let mut scratch = typio::core::registry::EngineRegistry::new();
        let validated = validator
            .load_single(&mut scratch, &path)
            .map_err(|_| SvcError)?;
        if validated.name != name {
            return Err(SvcError);
        }

        let registry = instance.registry_rust_mut().ok_or(SvcError)?;
        let was_active_keyboard = registry.active_keyboard_name() == Some(name);
        let was_active_voice = registry.active_voice_name() == Some(name);
        if registry.engine_info(name).is_none() {
            return Err(SvcError);
        }

        registry.unregister(name).map_err(|_| SvcError)?;
        let loaded = crate::engine_loader::EngineLoader::with_voice()
            .load_single(registry, &path)
            .map_err(|_| SvcError)?;

        if was_active_keyboard {
            registry
                .activate_keyboard(&loaded.name)
                .map_err(|_| SvcError)?;
        } else if was_active_voice {
            registry
                .activate_voice(&loaded.name)
                .map_err(|_| SvcError)?;
        }
        Ok(())
    }

    // ── daemon ──

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn runtime_state(&self) -> Option<RuntimeState> {
        None
    }
}

fn canonical_manifest_path(path: &Path, require_absolute: bool) -> Result<PathBuf, SvcError> {
    if (require_absolute && !path.is_absolute())
        || path.extension().and_then(|extension| extension.to_str()) != Some("toml")
    {
        return Err(SvcError);
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| SvcError)?;
    canonical.is_file().then_some(canonical).ok_or(SvcError)
}

impl TypioBackend {
    fn set_active_engine(&self, name: &str, voice: bool) -> Result<(), SvcError> {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return Err(SvcError);
        }
        let name_c = c_str(name)?;
        let result = if voice {
            typio::c_api::registry::typio_registry_set_active_voice(reg, name_c.as_ptr())
        } else {
            typio::c_api::registry::typio_registry_set_active_keyboard(reg, name_c.as_ptr())
        };
        match result {
            typio::TypioResult::TypioOk => Ok(()),
            _ => Err(SvcError),
        }
    }

    fn cycle_engine(&self, forward: bool, voice: bool) -> Result<(), SvcError> {
        let reg = self.registry_ptr();
        if reg.is_null() {
            return Err(SvcError);
        }
        let result = match (forward, voice) {
            (true, false) => typio::c_api::registry::typio_registry_next_keyboard(reg),
            (false, false) => typio::c_api::registry::typio_registry_prev_keyboard(reg),
            (true, true) => typio::c_api::registry::typio_registry_next_voice(reg),
            (false, true) => typio::c_api::registry::typio_registry_prev_voice(reg),
        };
        match result {
            typio::TypioResult::TypioOk => Ok(()),
            _ => Err(SvcError),
        }
    }
}

// ── RegistryView adapter for StateController ─────────────────────────────────

/// A [`RegistryView`] backed by the live [`TypioInstance`].
pub struct TypioRegistryView {
    instance: *mut typio::TypioInstance,
}

impl TypioRegistryView {
    /// The instance pointer must remain valid for the lifetime of the view.
    pub fn new(instance: *mut typio::TypioInstance) -> Self {
        Self { instance }
    }

    fn registry(&self) -> Option<&typio::core::registry::EngineRegistry> {
        unsafe { self.instance.as_ref() }.and_then(|i| i.registry_rust())
    }

    fn config_string(&self, key: &str) -> Option<String> {
        if self.instance.is_null() {
            return None;
        }
        let cfg = typio::instance::typio_instance_get_config(self.instance);
        if cfg.is_null() {
            return None;
        }
        let key_c = c_str(key).ok()?;
        let value = typio::config::typio_config_get_string(cfg, key_c.as_ptr(), ptr::null());
        if value.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        if s.is_empty() { None } else { Some(s) }
    }
}

impl RegistryView for TypioRegistryView {
    fn active_keyboard(&self) -> Option<String> {
        self.registry()
            .and_then(|r| r.active_keyboard_name())
            .map(str::to_string)
    }

    fn active_language(&self) -> Option<String> {
        self.registry()
            .and_then(|r| r.active_language())
            .map(str::to_string)
    }

    fn active_voice(&self) -> Option<String> {
        self.registry()
            .and_then(|r| r.active_voice_name())
            .map(str::to_string)
    }

    fn engine_display_name(&self, name: &str) -> Option<String> {
        self.registry()
            .and_then(|r| r.engine_info(name))
            .map(|info| info.display_name.clone())
    }

    fn config_icon(&self, key: &str) -> Option<String> {
        self.config_string(key)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn c_str(s: &str) -> Result<CString, SvcError> {
    CString::new(s).map_err(|_| SvcError)
}

fn parse_tip_request(json: &str) -> Result<(Request, i64), StandardError> {
    let value: Value = serde_json::from_str(json).map_err(|_| StandardError::ParseError)?;
    let request: Request =
        serde_json::from_value(value).map_err(|_| StandardError::InvalidRequest)?;
    if request.jsonrpc != crate::ipc::protocol::JSONRPC_VERSION {
        return Err(StandardError::InvalidRequest);
    }
    let Id::Number(id) = request.id else {
        return Err(StandardError::InvalidRequest);
    };
    Ok((request, id))
}

#[derive(Clone)]
struct SchemaField {
    field_type: FieldType,
    default: Value,
    label: Option<String>,
    section: Option<String>,
    choices: Option<Vec<String>>,
    min: i32,
    max: i32,
}

impl SchemaField {
    fn from_raw(raw: &typio::types::TypioConfigField) -> Option<Self> {
        use typio::types::TypioFieldType;

        let c_string = |ptr: *const std::ffi::c_char| {
            (!ptr.is_null()).then(|| {
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned()
            })
        };
        let (field_type, default) = match raw.type_ {
            TypioFieldType::TypioFieldString => {
                let ptr = unsafe { raw.def.s };
                (
                    FieldType::String,
                    Value::String(c_string(ptr).unwrap_or_default()),
                )
            }
            TypioFieldType::TypioFieldInt => {
                (FieldType::Int, Value::Number(unsafe { raw.def.i }.into()))
            }
            TypioFieldType::TypioFieldBool => (FieldType::Bool, Value::Bool(unsafe { raw.def.b })),
            TypioFieldType::TypioFieldFloat => (
                FieldType::Float,
                serde_json::Number::from_f64(unsafe { raw.def.f })
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
            ),
        };
        let choices = if raw.ui_options.is_null() {
            None
        } else {
            let mut values = Vec::new();
            let mut cursor = raw.ui_options;
            unsafe {
                while !(*cursor).is_null() {
                    values.push(CStr::from_ptr(*cursor).to_string_lossy().into_owned());
                    cursor = cursor.add(1);
                }
            }
            Some(values)
        };
        Some(Self {
            field_type,
            default,
            label: c_string(raw.ui_label),
            section: c_string(raw.ui_section),
            choices,
            min: raw.ui_min,
            max: raw.ui_max,
        })
    }

    fn descriptor(&self, key: String) -> ConfigField {
        ConfigField {
            key,
            field_type: self.field_type,
            label: self.label.clone(),
            section: self.section.clone(),
            choices: self.choices.clone(),
        }
    }
}

fn schema_field(key: &str) -> Option<SchemaField> {
    let key = CString::new(key).ok()?;
    let raw = typio::config_schema::typio_config_schema_find(key.as_ptr());
    unsafe { raw.as_ref() }.and_then(SchemaField::from_raw)
}

fn config_field_from_raw(raw: &typio::types::TypioConfigField) -> Option<ConfigField> {
    if raw.key.is_null() {
        return None;
    }
    let key = unsafe { CStr::from_ptr(raw.key) }
        .to_string_lossy()
        .into_owned();
    SchemaField::from_raw(raw).map(|field| field.descriptor(key))
}

fn schema_default(field: &SchemaField) -> Option<(Value, FieldType)> {
    Some((field.default.clone(), field.field_type))
}

fn config_value_type(value: &typio::config::ConfigValue) -> Option<FieldType> {
    use typio::config::ConfigValue;
    match value {
        ConfigValue::String(_) => Some(FieldType::String),
        ConfigValue::Int(_) => Some(FieldType::Int),
        ConfigValue::Bool(_) => Some(FieldType::Bool),
        ConfigValue::Float(_) => Some(FieldType::Float),
        ConfigValue::Array(_) => Some(FieldType::Array),
        ConfigValue::Object(_) => None,
    }
}

fn config_value_to_json(value: &typio::config::ConfigValue) -> Value {
    use typio::config::ConfigValue;
    match value {
        ConfigValue::String(value) => Value::String(value.to_string_lossy().into_owned()),
        ConfigValue::Int(value) => Value::Number((*value).into()),
        ConfigValue::Bool(value) => Value::Bool(*value),
        ConfigValue::Float(value) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ConfigValue::Array(values) => {
            Value::Array(values.iter().map(config_value_to_json).collect())
        }
        ConfigValue::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.to_string(), config_value_to_json(value)))
                .collect(),
        ),
    }
}

fn parse_string_list(raw: &str) -> Result<Vec<String>, ()> {
    let trimmed = raw.trim();
    if trimmed.starts_with('[') {
        let value: Value = serde_json::from_str(trimmed).map_err(|_| ())?;
        return value
            .as_array()
            .ok_or(())?
            .iter()
            .map(|item| item.as_str().map(str::to_string).ok_or(()))
            .collect();
    }
    Ok(trimmed
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect())
}

fn set_config_value(
    config: *mut typio::config::Config,
    key: &str,
    raw: &str,
    field_type: FieldType,
    schema: Option<&SchemaField>,
) -> Result<(), ()> {
    let key = CString::new(key).map_err(|_| ())?;
    let result = match field_type {
        FieldType::String => {
            if let Some(choices) = schema.and_then(|field| field.choices.as_ref())
                && !choices.iter().any(|choice| choice == raw)
            {
                return Err(());
            }
            let value = CString::new(raw).map_err(|_| ())?;
            typio::config::typio_config_set_string(config, key.as_ptr(), value.as_ptr())
        }
        FieldType::Int => {
            let value = raw.trim().parse::<i32>().map_err(|_| ())?;
            if let Some(field) = schema
                && field.max > field.min
                && !(field.min..=field.max).contains(&value)
            {
                return Err(());
            }
            typio::config::typio_config_set_int(config, key.as_ptr(), value)
        }
        FieldType::Bool => {
            let value = match raw.trim() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(()),
            };
            typio::config::typio_config_set_bool(config, key.as_ptr(), value)
        }
        FieldType::Float => {
            let value = raw.trim().parse::<f64>().map_err(|_| ())?;
            if !value.is_finite() {
                return Err(());
            }
            typio::config::typio_config_set_float(config, key.as_ptr(), value)
        }
        FieldType::Array => {
            let values: Vec<CString> = parse_string_list(raw)?
                .into_iter()
                .map(CString::new)
                .collect::<Result<_, _>>()
                .map_err(|_| ())?;
            let pointers: Vec<*const std::ffi::c_char> =
                values.iter().map(|value| value.as_ptr()).collect();
            typio::config::typio_config_set_string_array(
                config,
                key.as_ptr(),
                pointers.as_ptr(),
                pointers.len(),
            )
        }
    };
    (result == typio::TypioResult::TypioOk)
        .then_some(())
        .ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn c_str_rejects_nul() {
        assert!(c_str("foo\0bar").is_err());
    }

    #[test]
    fn string_list_parser_accepts_csv_and_json() {
        assert_eq!(parse_string_list("zh-Hans, en").unwrap(), ["zh-Hans", "en"]);
        assert_eq!(
            parse_string_list(r#"["zh-Hans", "en"]"#).unwrap(),
            ["zh-Hans", "en"]
        );
    }

    #[test]
    fn string_list_parser_rejects_non_string_json_items() {
        assert!(parse_string_list(r#"["en", 2]"#).is_err());
    }

    #[test]
    fn tip_request_rejects_valid_json_with_invalid_envelope() {
        assert_eq!(
            parse_tip_request("not json").unwrap_err(),
            StandardError::ParseError
        );
        assert_eq!(
            parse_tip_request(r#"{"jsonrpc":"1.0","id":1,"method":"hello"}"#).unwrap_err(),
            StandardError::InvalidRequest
        );
        assert_eq!(
            parse_tip_request(r#"{"jsonrpc":"2.0","id":"x","method":"hello"}"#).unwrap_err(),
            StandardError::InvalidRequest
        );
    }

    #[test]
    fn live_backend_reports_schema_types_and_sources() {
        let temp = tempdir().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut instance =
            typio::TypioInstance::new_rust(Some(path), Some(path), Some(path), Vec::new());
        instance.init_rust().unwrap();
        let mut backend = TypioBackend::new(instance.as_mut());

        match backend.config_get("notifications.enable").unwrap() {
            ConfigGetOutcome::Found {
                value,
                field_type,
                source,
            } => {
                assert_eq!(value, Value::Bool(true));
                assert_eq!(field_type, FieldType::Bool);
                assert_eq!(source, ConfigSource::Default);
            }
            ConfigGetOutcome::Unknown => panic!("schema field should exist"),
        }

        assert_eq!(
            backend.config_set("notifications.enable", "false"),
            Some(Ok(()))
        );
        match backend.config_get("notifications.enable").unwrap() {
            ConfigGetOutcome::Found { value, source, .. } => {
                assert_eq!(value, Value::Bool(false));
                assert_eq!(source, ConfigSource::User);
            }
            ConfigGetOutcome::Unknown => panic!("written field should exist"),
        }
        assert!(
            backend
                .config_set("notifications.enable", "maybe")
                .unwrap()
                .is_err()
        );
        assert!(backend.save_config().is_ok());

        let saved = std::fs::read_to_string(temp.path().join("core.toml")).unwrap();
        assert!(saved.contains("enable = false"));
        assert!(
            !saved.contains("cooldown_ms"),
            "defaults must not be persisted"
        );
    }

    #[test]
    fn live_backend_preserves_array_values() {
        let temp = tempdir().unwrap();
        std::fs::write(
            temp.path().join("core.toml"),
            "[languages]\nenabled = [\"zh-Hans\", \"en\"]\n",
        )
        .unwrap();
        let path = temp.path().to_str().unwrap();
        let mut instance =
            typio::TypioInstance::new_rust(Some(path), Some(path), Some(path), Vec::new());
        instance.init_rust().unwrap();
        let backend = TypioBackend::new(instance.as_mut());

        match backend.config_get("languages.enabled").unwrap() {
            ConfigGetOutcome::Found {
                value,
                field_type,
                source,
            } => {
                assert_eq!(value, serde_json::json!(["zh-Hans", "en"]));
                assert_eq!(field_type, FieldType::Array);
                assert_eq!(source, ConfigSource::User);
            }
            ConfigGetOutcome::Unknown => panic!("array field should exist"),
        }
    }

    #[test]
    fn live_backend_loads_and_prevalidates_engine_reloads() {
        let temp = tempdir().unwrap();
        let manifest = temp.path().join("typio-engine-fixture.toml");
        let valid_manifest = |display_name: &str| {
            format!(
                r#"
name = "fixture"
type = "keyboard"
protocol = "typio-engine-protocol"
display_name = "{display_name}"
command = "/nonexistent/typio-engine-fixture"
languages = ["und"]
"#
            )
        };
        std::fs::write(&manifest, valid_manifest("Fixture")).unwrap();

        let path = temp.path().to_str().unwrap();
        let mut instance = typio::TypioInstance::new_rust(
            Some(path),
            Some(path),
            Some(path),
            vec![path.to_string()],
        );
        instance.init_rust().unwrap();
        let mut backend = TypioBackend::new(instance.as_mut());

        assert!(backend.engine_load("relative.toml").is_err());
        assert!(backend.engine_load(manifest.to_str().unwrap()).is_ok());
        assert_eq!(backend.list_keyboards(), ["fixture"]);

        std::fs::write(&manifest, "not valid toml").unwrap();
        assert!(backend.engine_reload("fixture", None).is_err());
        assert_eq!(backend.list_keyboards(), ["fixture"]);

        std::fs::write(&manifest, valid_manifest("Fixture Reloaded")).unwrap();
        assert!(backend.engine_reload("fixture", None).is_ok());
        assert_eq!(
            backend.engine_display_name("fixture").as_deref(),
            Some("Fixture Reloaded")
        );
    }
}
