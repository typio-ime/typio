//! Owned framework and engine configuration schema registry.

use crate::config::{Config, ConfigValue};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Schema field type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFieldType {
    /// UTF-8 string.
    String,
    /// Signed 32-bit integer.
    Integer,
    /// Boolean.
    Boolean,
    /// Finite 64-bit float.
    Float,
}

/// Typed schema default value.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigDefault {
    /// UTF-8 string.
    String(String),
    /// Signed 32-bit integer.
    Integer(i32),
    /// Boolean.
    Boolean(bool),
    /// Finite 64-bit float.
    Float(f64),
}

/// Owned configuration schema field.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSchemaField {
    /// Fully-qualified dotted key.
    pub key: String,
    /// Typed default.
    pub default: ConfigDefault,
    /// Human-readable label.
    pub label: Option<String>,
    /// Settings section.
    pub section: Option<String>,
    /// Integer UI minimum.
    pub minimum: i32,
    /// Integer UI maximum.
    pub maximum: i32,
    /// Integer UI step.
    pub step: i32,
    /// Enumerated string choices.
    pub options: Vec<String>,
    /// Optional runtime-property identifier.
    pub runtime_property: Option<String>,
}

impl ConfigSchemaField {
    /// Type implied by the default value.
    pub const fn field_type(&self) -> ConfigFieldType {
        match self.default {
            ConfigDefault::String(_) => ConfigFieldType::String,
            ConfigDefault::Integer(_) => ConfigFieldType::Integer,
            ConfigDefault::Boolean(_) => ConfigFieldType::Boolean,
            ConfigDefault::Float(_) => ConfigFieldType::Float,
        }
    }
}

/// Schema replacement failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaError {
    InvalidField,
    AlreadyExists,
}

static ENGINE_FIELDS: LazyLock<RwLock<HashMap<String, Vec<ConfigSchemaField>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn field(
    key: &str,
    default: ConfigDefault,
    label: &str,
    section: &str,
    minimum: i32,
    maximum: i32,
    step: i32,
) -> ConfigSchemaField {
    ConfigSchemaField {
        key: key.into(),
        default,
        label: Some(label.into()),
        section: Some(section.into()),
        minimum,
        maximum,
        step,
        options: Vec::new(),
        runtime_property: None,
    }
}

fn framework_fields() -> Vec<ConfigSchemaField> {
    use ConfigDefault::{Boolean, Integer, String as Text};
    vec![
        field(
            "keyboard.per_app_preferences",
            Boolean(true),
            "Per-app preferences",
            "keyboard",
            0,
            0,
            0,
        ),
        field(
            "notifications.enable",
            Boolean(true),
            "Enable",
            "notifications",
            0,
            0,
            0,
        ),
        field(
            "notifications.startup_checks",
            Boolean(true),
            "Startup checks",
            "notifications",
            0,
            0,
            0,
        ),
        field(
            "notifications.runtime",
            Boolean(true),
            "Runtime alerts",
            "notifications",
            0,
            0,
            0,
        ),
        field(
            "notifications.voice",
            Boolean(true),
            "Voice alerts",
            "notifications",
            0,
            0,
            0,
        ),
        field(
            "notifications.cooldown_ms",
            Integer(15_000),
            "Cooldown (ms)",
            "notifications",
            0,
            300_000,
            1_000,
        ),
        field(
            "shortcuts.switch_language",
            Text("Ctrl+Shift".into()),
            "Switch language",
            "shortcuts",
            0,
            0,
            0,
        ),
        field(
            "shortcuts.switch_keyboard_engine",
            Text(String::new()),
            "Switch keyboard engine",
            "shortcuts",
            0,
            0,
            0,
        ),
        field(
            "languages.enabled",
            Text(String::new()),
            "Enabled languages",
            "languages",
            0,
            0,
            0,
        ),
        field(
            "shortcuts.exit",
            Text("Ctrl+Shift+Escape".into()),
            "Exit",
            "shortcuts",
            0,
            0,
            0,
        ),
        field(
            "shortcuts.voice_ptt",
            Text("Super+v".into()),
            "Voice (PTT)",
            "shortcuts",
            0,
            0,
            0,
        ),
        field(
            "keyboard.engine",
            Text(String::new()),
            "Keyboard engine",
            "keyboard",
            0,
            0,
            0,
        ),
        field(
            "keyboard.disabled",
            Text(String::new()),
            "Disabled keyboard engines",
            "keyboard",
            0,
            0,
            0,
        ),
        field(
            "voice.engine",
            Text(String::new()),
            "Voice engine",
            "voice",
            0,
            0,
            0,
        ),
        field(
            "voice.disabled",
            Text(String::new()),
            "Disabled voice engines",
            "voice",
            0,
            0,
            0,
        ),
    ]
}

/// Return an owned snapshot of framework and engine-published fields.
pub fn fields() -> Vec<ConfigSchemaField> {
    let mut result = framework_fields();
    let registry = ENGINE_FIELDS.read().expect("schema registry poisoned");
    let mut engines: Vec<_> = registry.iter().collect();
    engines.sort_by_key(|(name, _)| *name);
    for (_, fields) in engines {
        result.extend(fields.iter().cloned());
    }
    result
}

/// Look up one field and return an owned snapshot.
pub fn find(key: &str) -> Option<ConfigSchemaField> {
    fields().into_iter().find(|field| field.key == key)
}

/// Populate missing keys with non-empty schema defaults.
pub fn apply_defaults(config: &mut Config) {
    for field in fields() {
        if config.value(&field.key).is_some() {
            continue;
        }
        let value = match field.default {
            ConfigDefault::String(value) if value.is_empty() => continue,
            ConfigDefault::String(value) => ConfigValue::String(value),
            ConfigDefault::Integer(value) => ConfigValue::Int(value),
            ConfigDefault::Boolean(value) => ConfigValue::Bool(value),
            ConfigDefault::Float(value) => ConfigValue::Float(value),
        };
        config.set_default_value(field.key, value);
    }
}

/// Atomically replace the dynamic schema owned by one process engine.
pub(crate) fn replace_process_engine_schema(
    engine_name: &str,
    fields: &[ConfigSchemaField],
) -> Result<(), SchemaError> {
    let prefix = format!("engines.{engine_name}.");
    let mut seen = std::collections::HashSet::new();
    if fields.iter().any(|field| {
        field.key.len() == prefix.len()
            || !field.key.starts_with(&prefix)
            || !seen.insert(field.key.as_str())
    }) {
        return Err(SchemaError::InvalidField);
    }

    let framework = framework_fields();
    let mut registry = ENGINE_FIELDS.write().expect("schema registry poisoned");
    let collides = fields.iter().any(|field| {
        framework.iter().any(|base| base.key == field.key)
            || registry.iter().any(|(owner, existing)| {
                owner != engine_name && existing.iter().any(|item| item.key == field.key)
            })
    });
    if collides {
        return Err(SchemaError::AlreadyExists);
    }
    if fields.is_empty() {
        registry.remove(engine_name);
    } else {
        registry.insert(engine_name.to_string(), fields.to_vec());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_user_provenance() {
        let mut config = Config::new();
        apply_defaults(&mut config);
        assert_eq!(config.integer("notifications.cooldown_ms", 0), 15_000);
        assert!(!config.is_user_value("notifications.cooldown_ms"));
        assert!(config.value("shortcuts.switch_keyboard_engine").is_none());
    }

    #[test]
    fn display_fields_belong_to_frontends() {
        assert!(find("display.font_size").is_none());
    }
}
