use std::io;

use serde::Deserialize;
use serde_json::{Value, json};
use typio_client::Client;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub value: Value,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub section: String,
    #[serde(default)]
    pub choices: Vec<String>,
}

impl ConfigEntry {
    pub fn display_label(&self) -> &str {
        if self.label.is_empty() {
            &self.key
        } else {
            &self.label
        }
    }

    pub fn text_value(&self) -> String {
        value_as_text(&self.value)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EngineCommand {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Engine {
    pub name: String,
    pub kind: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub properties: Vec<ConfigEntry>,
    #[serde(default)]
    pub commands: Vec<EngineCommand>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Language {
    pub tag: String,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Default, Deserialize)]
struct LanguageList {
    #[serde(default)]
    languages: Vec<Language>,
    #[serde(default)]
    active: String,
}

#[derive(Debug, Default)]
pub struct Snapshot {
    pub daemon_version: String,
    pub engines: Vec<Engine>,
    pub languages: Vec<Language>,
    pub active_language: String,
    pub config: Vec<ConfigEntry>,
    pub daemon_status: Value,
}

impl Snapshot {
    pub fn load() -> io::Result<Self> {
        let mut client = Client::connect()?;
        let hello = client.call("hello", json!({}))?;
        let protocol = hello
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if protocol < 3 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("Typio daemon protocol {protocol} is too old (need 3)"),
            ));
        }

        let mut engines: Vec<Engine> = decode(client.call("engine.list", json!({}))?)?;
        for engine in &mut engines {
            let detail: Engine =
                decode(client.call("engine.describe", json!({ "name": engine.name }))?)?;
            engine.display_name = detail.display_name;
            engine.properties = detail.properties;
            engine.commands = detail.commands;
        }

        let language_list: LanguageList = decode(client.call("language.list", json!({}))?)?;
        let config = decode(client.call("config.list", json!({ "prefix": "" }))?)?;
        let daemon_status = client.call("daemon.status", json!({}))?;

        Ok(Self {
            daemon_version: hello
                .get("daemonVersion")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            engines,
            languages: language_list.languages,
            active_language: language_list.active,
            config,
            daemon_status,
        })
    }
}

pub fn rpc(method: &str, params: Value) -> io::Result<Value> {
    Client::connect()?.call(method, params)
}

pub fn value_as_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> io::Result<T> {
    serde_json::from_value(value).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_entry_decodes_tip_shape() {
        let entry: ConfigEntry = decode(json!({
            "key": "shortcuts.exit",
            "type": "string",
            "value": "Control+Alt+Escape",
            "source": "default",
            "label": "Emergency exit",
            "section": "shortcuts"
        }))
        .unwrap();

        assert_eq!(entry.field_type, "string");
        assert_eq!(entry.text_value(), "Control+Alt+Escape");
    }
}
