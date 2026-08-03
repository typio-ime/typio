//! Owned Typio configuration tree.

mod parse;
mod serialize;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONFIG_READS_IN_FLIGHT: usize = 4;
static CONFIG_READS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// A single configuration value.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigValue {
    /// UTF-8 string.
    String(String),
    /// Signed 32-bit integer.
    Int(i32),
    /// Boolean.
    Bool(bool),
    /// 64-bit floating point.
    Float(f64),
    /// Ordered array.
    Array(Vec<ConfigValue>),
    /// Nested configuration object.
    Object(Box<Config>),
}

/// Dotted-key configuration with user/default provenance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub(crate) entries: HashMap<String, ConfigValue>,
    pub(crate) user_keys: HashSet<String>,
}

/// Configuration load, parse, or persistence error.
#[derive(Debug)]
pub enum ConfigError {
    /// A file could not be read or written.
    Io(std::io::Error),
    /// Input was neither TOML nor the supported legacy subset.
    Parse,
    /// The bounded file-read budget expired.
    Timeout,
    /// Too many timed-out reads are still in flight.
    Busy,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Parse => formatter.write_str("invalid configuration text"),
            Self::Timeout => formatter.write_str("configuration read timed out"),
            Self::Busy => formatter.write_str("too many configuration reads are in flight"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl Config {
    /// Create an empty configuration tree.
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_value(&mut self, key: String, value: ConfigValue) {
        self.user_keys.insert(key.clone());
        self.entries.insert(key, value);
    }

    pub(crate) fn set_default_value(&mut self, key: String, value: ConfigValue) {
        self.entries.insert(key, value);
    }

    /// Borrow a dotted-key value.
    pub fn value(&self, key: &str) -> Option<&ConfigValue> {
        self.entries.get(key)
    }

    /// Iterate over dotted keys and values.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ConfigValue)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    /// Whether a value came from user input rather than a schema default.
    pub fn is_user_value(&self, key: &str) -> bool {
        self.user_keys.contains(key)
    }

    /// Parse TOML, falling back to the supported legacy INI-like subset.
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        match content.parse::<toml::Value>() {
            Ok(value) => Ok(Self::from_toml(&value, "")),
            Err(_) => parse::parse_ini_like(content).ok_or(ConfigError::Parse),
        }
    }

    /// Load a file with a bounded blocking-read budget.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if CONFIG_READS_IN_FLIGHT
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_CONFIG_READS_IN_FLIGHT).then_some(count + 1)
            })
            .is_err()
        {
            return Err(ConfigError::Busy);
        }
        let path = path.to_path_buf();
        let (sender, receiver) = mpsc::channel();
        if thread::Builder::new()
            .name("typio-config-read".into())
            .spawn(move || {
                let result = fs::read_to_string(path);
                CONFIG_READS_IN_FLIGHT.fetch_sub(1, Ordering::Release);
                let _ = sender.send(result);
            })
            .is_err()
        {
            CONFIG_READS_IN_FLIGHT.fetch_sub(1, Ordering::Release);
            return Err(ConfigError::Busy);
        }
        match receiver.recv_timeout(CONFIG_READ_TIMEOUT) {
            Ok(Ok(content)) => Self::parse(&content),
            Ok(Err(error)) => Err(ConfigError::Io(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ConfigError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ConfigError::Busy),
        }
    }

    /// Serialize user-provided values as TOML.
    pub fn to_toml(&self) -> String {
        serialize::config_to_string_internal(self)
    }

    /// Save atomically using write, sync, and rename.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        let temporary = path.with_extension("tmp");
        let result = (|| -> Result<(), std::io::Error> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(self.to_toml().as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(ConfigError::Io)
    }

    /// Borrow a string or return `default`.
    pub fn string<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        match self.entries.get(key) {
            Some(ConfigValue::String(value)) => value,
            _ => default,
        }
    }

    /// Read an integer, accepting an integral conversion from float.
    pub fn integer(&self, key: &str, default: i32) -> i32 {
        match self.entries.get(key) {
            Some(ConfigValue::Int(value)) => *value,
            Some(ConfigValue::Float(value)) => *value as i32,
            _ => default,
        }
    }

    /// Read a boolean.
    pub fn boolean(&self, key: &str, default: bool) -> bool {
        match self.entries.get(key) {
            Some(ConfigValue::Bool(value)) => *value,
            _ => default,
        }
    }

    /// Read a float, accepting an integer conversion.
    pub fn float(&self, key: &str, default: f64) -> f64 {
        match self.entries.get(key) {
            Some(ConfigValue::Float(value)) => *value,
            Some(ConfigValue::Int(value)) => f64::from(*value),
            _ => default,
        }
    }

    /// Store a typed user value.
    pub fn set(&mut self, key: impl Into<String>, value: ConfigValue) {
        self.set_value(key.into(), value);
    }

    /// Store a UTF-8 string value.
    pub fn set_string(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.set_value(key.into(), ConfigValue::String(value.into()));
    }

    /// Remove a key and its provenance marker.
    pub fn remove(&mut self, key: &str) -> bool {
        self.user_keys.remove(key);
        self.entries.remove(key).is_some()
    }

    fn from_toml(value: &toml::Value, prefix: &str) -> Self {
        let mut config = Self::new();
        config.populate_from_toml(value, prefix);
        config
    }

    fn populate_from_toml(&mut self, value: &toml::Value, prefix: &str) {
        match value {
            toml::Value::Table(table) => {
                for (key, value) in table {
                    let full_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    if value.is_table() {
                        self.populate_from_toml(value, &full_key);
                    } else if let Some(value) = parse::toml_to_config_value(value) {
                        self.set_value(full_key, value);
                    }
                }
            }
            _ => {
                if let Some(value) = parse::toml_to_config_value(value) {
                    self.set_value(
                        if prefix.is_empty() { "value" } else { prefix }.to_string(),
                        value,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_serializes_owned_strings() {
        let config = Config::parse("[engine]\nname = \"compose\"\n").unwrap();
        assert_eq!(config.string("engine.name", ""), "compose");
        assert!(config.to_toml().contains("name = \"compose\""));
    }

    #[test]
    fn invalid_legacy_input_is_rejected() {
        assert!(matches!(
            Config::parse("not a setting"),
            Err(ConfigError::Parse)
        ));
    }
}
