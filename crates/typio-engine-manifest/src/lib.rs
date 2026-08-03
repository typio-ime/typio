//! Typed contract for `typio-engine-*.toml` process manifests.
//!
//! The manifest is shared by discovery, conformance tooling, and engine
//! packages. It describes how to start an isolated engine process; it does not
//! expose an in-process implementation ABI.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Protocol identifier required in current engine manifests.
pub const ENGINE_PROTOCOL: &str = "typio-engine-protocol";

/// Default language code used when neither language field is present.
pub const DEFAULT_LANGUAGE: &str = "und";

/// Parsed representation of a `typio-engine-*.toml` manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineManifest {
    /// Machine-readable engine identifier.
    pub name: String,
    /// Engine category: `keyboard` or `voice`.
    #[serde(rename = "type")]
    pub engine_type: String,
    /// Wire protocol spoken by the engine process.
    pub protocol: String,
    /// Binary to execute, optionally relative to the manifest directory.
    pub command: Option<String>,
    /// Human-readable name.
    pub display_name: Option<String>,
    /// Free-form description.
    pub description: Option<String>,
    /// Author attribution.
    pub author: Option<String>,
    /// Freedesktop icon name.
    pub icon: Option<String>,
    /// Legacy single-language declaration.
    pub language: Option<String>,
    /// Ordered language list, primary first.
    pub languages: Option<Vec<String>>,
    /// Single-argument form, placed before `args`.
    pub arg: Option<String>,
    /// Array-form process arguments.
    pub args: Option<Vec<String>>,
    /// Capabilities that the host must provide.
    pub required: Option<Vec<String>>,
    /// Capabilities that the engine can use when available.
    pub optional: Option<Vec<String>>,
}

impl EngineManifest {
    /// Parse a manifest from TOML text.
    pub fn parse(toml_text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_text)
    }

    /// Read and parse a manifest from disk.
    pub fn read_from(path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| ManifestError::Read(path.to_path_buf(), error))?;
        Self::parse(&text).map_err(|error| ManifestError::Parse(path.to_path_buf(), error))
    }

    /// Whether all required manifest fields are present and non-empty.
    pub fn has_required_fields(&self) -> bool {
        !self.name.is_empty()
            && !self.engine_type.is_empty()
            && !self.protocol.is_empty()
            && self
                .command
                .as_deref()
                .is_some_and(|value| !value.is_empty())
    }

    /// Resolve the engine command and arguments against the manifest path.
    pub fn argv(&self, manifest_path: &Path) -> Result<Vec<String>, ManifestError> {
        let command = self
            .command
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ManifestError::MissingCommand(manifest_path.to_path_buf()))?;
        let mut argv = Vec::with_capacity(1 + self.args.as_ref().map_or(0, Vec::len));
        argv.push(resolve_path_arg(manifest_path, command));
        if let Some(argument) = self.arg.as_deref().filter(|value| !value.is_empty()) {
            argv.push(resolve_path_arg(manifest_path, argument));
        }
        if let Some(arguments) = self.args.as_ref() {
            argv.extend(
                arguments
                    .iter()
                    .filter(|value| !value.is_empty())
                    .map(|value| resolve_path_arg(manifest_path, value)),
            );
        }
        Ok(argv)
    }

    /// Ordered language list with the legacy and default fallbacks applied.
    pub fn effective_languages(&self) -> Vec<String> {
        if let Some(languages) = self.languages.as_ref().filter(|value| !value.is_empty()) {
            return languages.clone();
        }
        if let Some(language) = self.language.as_ref().filter(|value| !value.is_empty()) {
            return vec![language.clone()];
        }
        vec![DEFAULT_LANGUAGE.to_string()]
    }

    /// Primary language after applying manifest fallbacks.
    pub fn primary_language(&self) -> String {
        self.effective_languages()
            .into_iter()
            .next()
            .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string())
    }
}

/// Error reading or resolving an engine manifest.
#[derive(Debug)]
pub enum ManifestError {
    /// The manifest could not be read.
    Read(PathBuf, std::io::Error),
    /// The manifest was not valid TOML.
    Parse(PathBuf, toml::de::Error),
    /// The required command was absent or empty.
    MissingCommand(PathBuf),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(path, error) => {
                write!(
                    formatter,
                    "cannot read manifest {}: {error}",
                    path.display()
                )
            }
            Self::Parse(path, error) => write!(
                formatter,
                "manifest {} is not valid TOML: {error}",
                path.display()
            ),
            Self::MissingCommand(path) => write!(
                formatter,
                "manifest {} is missing required `command` field",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Resolve a command or argument using manifest-relative path semantics.
pub fn resolve_path_arg(manifest_path: &Path, value: &str) -> String {
    if value.is_empty() || value.starts_with('/') || !value.contains('/') {
        return value.to_string();
    }
    manifest_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(value)
        .to_string_lossy()
        .into_owned()
}

/// Whether a filename is a source engine manifest rather than an installed copy.
pub fn is_manifest_filename(name: &str) -> bool {
    name.starts_with("typio-engine-")
        && name.ends_with(".toml")
        && !name.ends_with(".installed.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
name = "rime"
type = "keyboard"
protocol = "typio-engine-protocol"
display_name = "Rime"
icon = "typio-rime-symbolic"
language = "zh"
languages = ["zh", "yue"]
command = "./typio-engine-rime"
args = ["--user-data", "./data"]
required = ["preedit", "candidates"]
optional = ["prediction"]
"#;

    #[test]
    fn parses_and_resolves_complete_manifest() {
        let manifest = EngineManifest::parse(SAMPLE).unwrap();
        assert!(manifest.has_required_fields());
        assert_eq!(manifest.effective_languages(), vec!["zh", "yue"]);
        assert_eq!(manifest.primary_language(), "zh");
        assert_eq!(
            manifest
                .argv(Path::new("/etc/typio/engines/typio-engine-rime.toml"))
                .unwrap(),
            vec![
                "/etc/typio/engines/./typio-engine-rime",
                "--user-data",
                "/etc/typio/engines/./data",
            ]
        );
    }

    #[test]
    fn argv_concatenates_arg_then_args_and_preserves_bare_values() {
        let manifest = EngineManifest::parse(
            r#"
name = "x"
type = "voice"
protocol = "typio-engine-protocol"
command = "x"
arg = "--first"
args = ["/absolute", "./relative"]
"#,
        )
        .unwrap();
        assert_eq!(
            manifest.argv(Path::new("/engines/x.toml")).unwrap(),
            vec!["x", "--first", "/absolute", "/engines/./relative"]
        );
    }

    #[test]
    fn language_and_filename_fallbacks_are_stable() {
        let mut manifest = EngineManifest::parse(SAMPLE).unwrap();
        manifest.languages = Some(Vec::new());
        assert_eq!(manifest.effective_languages(), vec!["zh"]);
        manifest.language = None;
        assert_eq!(manifest.effective_languages(), vec![DEFAULT_LANGUAGE]);
        assert!(is_manifest_filename("typio-engine-rime.toml"));
        assert!(!is_manifest_filename("typio-engine-rime.installed.toml"));
    }
}
