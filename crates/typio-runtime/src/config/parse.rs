//! Parsing helpers for TOML configuration files.

use super::{Config, ConfigValue};
pub(super) fn toml_to_config_value(v: &toml::Value) -> Option<ConfigValue> {
    match v {
        toml::Value::String(s) => Some(ConfigValue::String(s.clone())),
        toml::Value::Integer(i) => Some(ConfigValue::Int(*i as i32)),
        toml::Value::Float(f) => Some(ConfigValue::Float(*f)),
        toml::Value::Boolean(b) => Some(ConfigValue::Bool(*b)),
        toml::Value::Array(arr) => {
            let items: Vec<ConfigValue> = arr.iter().filter_map(toml_to_config_value).collect();
            Some(ConfigValue::Array(items))
        }
        toml::Value::Table(table) => {
            let mut cfg = Config::new();
            for (k, v) in table.iter() {
                if let Some(cv) = toml_to_config_value(v) {
                    cfg.entries.insert(k.clone(), cv);
                }
            }
            Some(ConfigValue::Object(Box::new(cfg)))
        }
        toml::Value::Datetime(_) => None,
    }
}
