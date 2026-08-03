//! UDS request bus — wires [`UdsServer`] to [`StatusService`] and broadcasts
//! state changes to subscribed clients.
//!
//! Port of `src/ipc/ipc_bus.c`. The C module couples UDS framing, request
//! dispatch, and runtime state mutation in one file. This Rust port keeps the
//! already-ported [`UdsServer`] and [`StatusService`] separate and only adds
//! the thin gluing layer plus a typio-core-backed [`ServiceBackend`] impl.
//!
//! ## Responsibilities
//!
//! - Install a request handler on the [`UdsServer`] that parses JSON-RPC,
//!   dispatches through [`StatusService`], and forwards any subscription change
//!   back to the server.
//! - Provide [`IpcBus::emit`] so a [`StateController`](crate::state_controller)
//!   listener can push notifications to subscribed UDS clients.
//! - Implement [`ServiceBackend`] for an owned shared [`TypioInstance`] so the
//!   generic dispatch service can drive live runtime state.

use std::cell::RefCell;
use std::collections::HashSet;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::ipc::framing::{Id, Request, Response, StandardError};
use crate::runtime::SharedInstance;
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

// ── typio-core-backed ServiceBackend ─────────────────────────────────────────

/// A [`ServiceBackend`] that drives the main-loop-owned Typio runtime.
pub struct TypioBackend {
    instance: SharedInstance,
}

impl TypioBackend {
    /// Share the daemon runtime with the synchronous UDS service.
    pub fn new(instance: SharedInstance) -> Self {
        Self { instance }
    }

    fn with_registry<R>(
        &self,
        operation: impl FnOnce(&typio::core::registry::EngineRegistry) -> R,
    ) -> Option<R> {
        let instance = self.instance.borrow();
        instance
            .registry_rust()
            .map(|registry| operation(&registry))
    }

    fn with_registry_mut<R>(
        &self,
        operation: impl FnOnce(&mut typio::core::registry::EngineRegistry) -> R,
    ) -> Option<R> {
        let instance = self.instance.borrow();
        instance
            .registry_rust_mut()
            .map(|mut registry| operation(&mut registry))
    }
}

impl ServiceBackend for TypioBackend {
    // ── config ──

    fn config_get(&self, key: &str) -> Option<ConfigGetOutcome> {
        let instance = self.instance.borrow();
        let cfg = instance.config_rust()?;
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
        let mut instance = self.instance.borrow_mut();
        let cfg = instance.config_rust_mut()?;
        let schema = schema_field(key);
        let current_type = cfg.value(key).and_then(config_value_type);
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
        let mut instance = self.instance.borrow_mut();
        let cfg = instance.config_rust_mut()?;
        let has_schema = schema_field(key).is_some();
        if cfg.remove(key) {
            typio::config_schema::apply_defaults(cfg);
            Some(Ok(()))
        } else if has_schema {
            Some(Ok(()))
        } else {
            Some(Err(SvcError))
        }
    }

    fn config_list(&self, prefix: &str) -> Option<Vec<ConfigEntry>> {
        let instance = self.instance.borrow();
        let cfg = instance.config_rust()?;
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for schema in typio::config_schema::fields() {
            let schema_field = SchemaField::from_owned(&schema);
            let field = schema_field.descriptor(schema.key.clone());
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
                schema_default(&schema_field)?
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
        self.instance.borrow().config_text().unwrap_or_default()
    }

    fn config_reload(&mut self) -> Result<(), SvcError> {
        self.instance
            .borrow_mut()
            .reload_config_rust()
            .map_err(|_| SvcError)
    }

    fn save_config(&mut self) -> Result<(), SvcError> {
        self.instance
            .borrow()
            .save_config_rust()
            .map_err(|_| SvcError)
    }

    fn notify_engine_config(&mut self, engine: &str, key: &str, value: &str) {
        let _ =
            self.with_registry_mut(|registry| registry.notify_config_change(engine, key, value));
    }

    // ── registry ──

    fn registry_present(&self) -> bool {
        self.with_registry(|_| ()).is_some()
    }

    fn list_keyboards(&self) -> Vec<String> {
        self.with_registry(|r| r.list_keyboards().into_iter().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn list_voices(&self) -> Vec<String> {
        self.with_registry(|r| r.list_voices().into_iter().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn list_languages(&self) -> Vec<String> {
        self.with_registry(|r| r.known_languages())
            .unwrap_or_default()
    }

    fn engine_info(&self, name: &str) -> Option<EngineKind> {
        self.with_registry(|r| {
            r.engine_info(name).map(|info| match info.engine_type {
                typio::core::engine::EngineType::Keyboard => EngineKind::Keyboard,
                typio::core::engine::EngineType::Voice => EngineKind::Voice,
            })
        })
        .flatten()
    }

    fn engine_display_name(&self, name: &str) -> Option<String> {
        self.with_registry(|r| r.engine_info(name).map(|info| info.display_name.clone()))
            .flatten()
    }

    fn active_keyboard(&self) -> Option<String> {
        self.with_registry(|r| r.active_keyboard_name().map(str::to_string))
            .flatten()
    }

    fn active_voice(&self) -> Option<String> {
        self.with_registry(|r| r.active_voice_name().map(str::to_string))
            .flatten()
    }

    fn active_language(&self) -> Option<String> {
        self.with_registry(|r| r.active_language().map(str::to_string))
            .flatten()
    }

    fn set_active_keyboard(&mut self, name: &str) -> Result<(), SvcError> {
        self.set_active_engine(name, false)
    }

    fn set_active_voice(&mut self, name: &str) -> Result<(), SvcError> {
        self.set_active_engine(name, true)
    }

    fn set_active_language(&mut self, tag: &str) -> Result<(), SvcError> {
        self.instance
            .borrow_mut()
            .activate_language(tag)
            .map_err(|_| SvcError)
    }

    fn cycle_keyboard(&mut self, forward: bool) -> Result<(), SvcError> {
        self.cycle_engine(forward, false)
    }

    fn cycle_voice(&mut self, forward: bool) -> Result<(), SvcError> {
        self.cycle_engine(forward, true)
    }

    fn cycle_language(&mut self, forward: bool) -> CycleLanguageOutcome {
        let direction = if forward {
            typio::core::registry::SwitchDirection::Next
        } else {
            typio::core::registry::SwitchDirection::Previous
        };
        let result = { self.instance.borrow_mut().cycle_language(direction) };
        match result {
            Ok(()) => CycleLanguageOutcome::Ok(self.active_language()),
            Err(typio::core::engine::EngineError::NotFound) => CycleLanguageOutcome::NoLanguages,
            Err(_) => CycleLanguageOutcome::Failed,
        }
    }

    fn list_commands(&self, name: &str) -> Vec<EngineCommand> {
        self.with_registry_mut(|registry| registry.list_commands(name))
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|command| EngineCommand {
                id: command.id,
                label: command.label,
            })
            .collect()
    }

    fn invoke_command(&mut self, name: &str, cmd: &str) -> InvokeOutcome {
        match self.with_registry_mut(|registry| registry.invoke_command(name, cmd)) {
            Some(Ok(())) => InvokeOutcome::Ok,
            Some(Err(typio::core::engine::EngineError::NotFound)) => InvokeOutcome::NotFound,
            Some(Err(typio::core::engine::EngineError::NotSupported)) => {
                InvokeOutcome::NotSupported
            }
            _ => InvokeOutcome::Failed,
        }
    }

    // ── engine loader ──
    //
    fn engine_load(&mut self, path: &str) -> Result<(), SvcError> {
        let path = canonical_manifest_path(Path::new(path), true)?;
        let instance = self.instance.borrow();
        let mut registry = instance.registry_rust_mut().ok_or(SvcError)?;
        crate::engine_loader::EngineLoader::with_voice()
            .load_single(&mut registry, &path)
            .map(|_| ())
            .map_err(|_| SvcError)
    }

    fn engine_unload(&mut self, name: &str) -> Result<(), SvcError> {
        self.with_registry_mut(|registry| registry.unregister(name))
            .ok_or(SvcError)?
            .map_err(|_| SvcError)
    }

    fn engine_reload(&mut self, name: &str, path: Option<&str>) -> Result<(), SvcError> {
        let instance = self.instance.borrow();
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

        let mut registry = instance.registry_rust_mut().ok_or(SvcError)?;
        let was_active_keyboard = registry.active_keyboard_name() == Some(name);
        let was_active_voice = registry.active_voice_name() == Some(name);
        if registry.engine_info(name).is_none() {
            return Err(SvcError);
        }

        registry.unregister(name).map_err(|_| SvcError)?;
        let loaded = crate::engine_loader::EngineLoader::with_voice()
            .load_single(&mut registry, &path)
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
        self.with_registry_mut(|registry| {
            if voice {
                registry.activate_voice(name)
            } else {
                registry.activate_keyboard(name)
            }
        })
        .ok_or(SvcError)?
        .map_err(|_| SvcError)
    }

    fn cycle_engine(&self, forward: bool, voice: bool) -> Result<(), SvcError> {
        let direction = if forward {
            typio::core::registry::SwitchDirection::Next
        } else {
            typio::core::registry::SwitchDirection::Previous
        };
        self.with_registry_mut(|registry| {
            if voice {
                registry.switch_voice(direction)
            } else {
                registry.switch_keyboard(direction)
            }
        })
        .ok_or(SvcError)?
        .map_err(|_| SvcError)
    }
}

// ── RegistryView adapter for StateController ─────────────────────────────────

/// A [`RegistryView`] backed by the live [`TypioInstance`].
pub struct TypioRegistryView {
    instance: SharedInstance,
}

impl TypioRegistryView {
    /// Share the live main-loop runtime.
    pub fn new(instance: SharedInstance) -> Self {
        Self { instance }
    }

    fn with_registry<R>(
        &self,
        operation: impl FnOnce(&typio::core::registry::EngineRegistry) -> R,
    ) -> Option<R> {
        let instance = self.instance.borrow();
        instance
            .registry_rust()
            .map(|registry| operation(&registry))
    }

    fn config_string(&self, key: &str) -> Option<String> {
        let instance = self.instance.borrow();
        let s = instance.config_rust()?.string(key, "").to_string();
        if s.is_empty() { None } else { Some(s) }
    }
}

impl RegistryView for TypioRegistryView {
    fn active_keyboard(&self) -> Option<String> {
        self.with_registry(|r| r.active_keyboard_name().map(str::to_string))
            .flatten()
    }

    fn active_language(&self) -> Option<String> {
        self.with_registry(|r| r.active_language().map(str::to_string))
            .flatten()
    }

    fn active_voice(&self) -> Option<String> {
        self.with_registry(|r| r.active_voice_name().map(str::to_string))
            .flatten()
    }

    fn engine_display_name(&self, name: &str) -> Option<String> {
        self.with_registry(|r| r.engine_info(name).map(|info| info.display_name.clone()))
            .flatten()
    }

    fn config_icon(&self, key: &str) -> Option<String> {
        self.config_string(key)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

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
    fn from_owned(raw: &typio::config_schema::ConfigSchemaField) -> Self {
        use typio::config_schema::ConfigDefault;
        let (field_type, default) = match &raw.default {
            ConfigDefault::String(value) => (FieldType::String, Value::String(value.clone())),
            ConfigDefault::Integer(value) => (FieldType::Int, Value::Number((*value).into())),
            ConfigDefault::Boolean(value) => (FieldType::Bool, Value::Bool(*value)),
            ConfigDefault::Float(value) => (
                FieldType::Float,
                serde_json::Number::from_f64(*value)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
            ),
        };
        Self {
            field_type,
            default,
            label: raw.label.clone(),
            section: raw.section.clone(),
            choices: (!raw.options.is_empty()).then(|| raw.options.clone()),
            min: raw.minimum,
            max: raw.maximum,
        }
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
    typio::config_schema::find(key).map(|field| SchemaField::from_owned(&field))
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
        ConfigValue::String(value) => Value::String(value.clone()),
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
    config: &mut typio::config::Config,
    key: &str,
    raw: &str,
    field_type: FieldType,
    schema: Option<&SchemaField>,
) -> Result<(), ()> {
    match field_type {
        FieldType::String => {
            if let Some(choices) = schema.and_then(|field| field.choices.as_ref())
                && !choices.iter().any(|choice| choice == raw)
            {
                return Err(());
            }
            config.set_string(key, raw);
        }
        FieldType::Int => {
            let value = raw.trim().parse::<i32>().map_err(|_| ())?;
            if let Some(field) = schema
                && field.max > field.min
                && !(field.min..=field.max).contains(&value)
            {
                return Err(());
            }
            config.set(key, typio::config::ConfigValue::Int(value));
        }
        FieldType::Bool => {
            let value = match raw.trim() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(()),
            };
            config.set(key, typio::config::ConfigValue::Bool(value));
        }
        FieldType::Float => {
            let value = raw.trim().parse::<f64>().map_err(|_| ())?;
            if !value.is_finite() {
                return Err(());
            }
            config.set(key, typio::config::ConfigValue::Float(value));
        }
        FieldType::Array => {
            let values = parse_string_list(raw)?;
            config.set(
                key,
                typio::config::ConfigValue::Array(
                    values
                        .into_iter()
                        .map(typio::config::ConfigValue::String)
                        .collect(),
                ),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
        let instance = Rc::new(RefCell::new(instance));
        let mut backend = TypioBackend::new(instance);

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
        let instance = Rc::new(RefCell::new(instance));
        let backend = TypioBackend::new(instance);

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
        let instance = Rc::new(RefCell::new(instance));
        let mut backend = TypioBackend::new(instance);

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
