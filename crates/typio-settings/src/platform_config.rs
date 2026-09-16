use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, value};

pub const DEFAULT_THEME: &str = "auto";
pub const DEFAULT_LAYOUT: &str = "horizontal";
pub const DEFAULT_FONT_SIZE: f64 = 11.0;
pub const DEFAULT_FONT_FAMILY: &str = "default";
pub const DEFAULT_ANCHOR_TIMEOUT_MS: i64 = 150;

/// Accepted `display.font_family` values, in menu order.
///
/// The text stack resolves faces through fontconfig, which exposes a family
/// *class* rather than an arbitrary family name; this list is the whole set of
/// values that actually change rendering.
pub const FONT_FAMILIES: [&str; 4] = ["default", "sans", "serif", "mono"];

pub struct DisplaySettings<'a> {
    pub theme: &'a str,
    pub layout: &'a str,
    pub font_size: f64,
    pub font_family: &'a str,
    pub panel_mode_indicator: bool,
    pub anchor_probe: bool,
    pub anchor_probe_timeout_ms: i64,
}

pub struct PlatformConfig {
    path: PathBuf,
    document: DocumentMut,
}

impl PlatformConfig {
    pub fn load() -> io::Result<Self> {
        Self::load_from(platform_config_path())
    }

    pub fn load_from(path: PathBuf) -> io::Result<Self> {
        let document = match fs::read_to_string(&path) {
            Ok(text) => text.parse::<DocumentMut>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("cannot parse {}: {error}", path.display()),
                )
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => DocumentMut::new(),
            Err(error) => return Err(error),
        };
        Ok(Self { path, document })
    }

    pub fn theme(&self) -> &str {
        self.string("panel_theme").unwrap_or(DEFAULT_THEME)
    }

    pub fn candidate_layout(&self) -> &str {
        self.string("candidate_layout").unwrap_or(DEFAULT_LAYOUT)
    }

    pub fn font_size(&self) -> f64 {
        self.float("font_size")
            .or_else(|| self.integer("font_size").map(|value| value as f64))
            .unwrap_or(DEFAULT_FONT_SIZE)
    }

    /// The configured family class. An unset or unrecognised value reads back
    /// as the default class, matching how the daemon interprets the key.
    pub fn font_family(&self) -> &str {
        let value = self.string("font_family").unwrap_or(DEFAULT_FONT_FAMILY);
        FONT_FAMILIES
            .into_iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(value))
            .unwrap_or(DEFAULT_FONT_FAMILY)
    }

    pub fn panel_mode_indicator(&self) -> bool {
        self.boolean("panel_mode_indicator").unwrap_or(false)
    }

    pub fn anchor_probe(&self) -> bool {
        self.boolean("anchor_probe").unwrap_or(true)
    }

    pub fn anchor_probe_timeout_ms(&self) -> i64 {
        self.integer("anchor_probe_timeout_ms")
            .unwrap_or(DEFAULT_ANCHOR_TIMEOUT_MS)
    }

    pub fn update(&mut self, settings: &DisplaySettings<'_>) {
        self.ensure_display_table();
        self.document["display"]["panel_theme"] = value(settings.theme);
        self.document["display"]["candidate_layout"] = value(settings.layout);
        self.document["display"]["font_size"] = value(settings.font_size);
        self.document["display"]["font_family"] = value(settings.font_family);
        self.document["display"]["panel_mode_indicator"] = value(settings.panel_mode_indicator);
        self.document["display"]["anchor_probe"] = value(settings.anchor_probe);
        self.document["display"]["anchor_probe_timeout_ms"] =
            value(settings.anchor_probe_timeout_ms);
    }

    pub fn save(&self) -> io::Result<()> {
        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "platform config has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("platform.toml");
        let temporary = parent.join(format!(".{file_name}.tmp"));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(self.document.to_string().as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &self.path)?;
        if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    fn ensure_display_table(&mut self) {
        if !self.document.get("display").is_some_and(Item::is_table) {
            self.document["display"] = Item::Table(Table::new());
        }
    }

    fn display(&self, key: &str) -> Option<&Item> {
        self.document.get("display")?.get(key)
    }

    fn string(&self, key: &str) -> Option<&str> {
        self.display(key)?.as_str()
    }

    fn boolean(&self, key: &str) -> Option<bool> {
        self.display(key)?.as_bool()
    }

    fn integer(&self, key: &str) -> Option<i64> {
        self.display(key)?.as_integer()
    }

    fn float(&self, key: &str) -> Option<f64> {
        self.display(key)?.as_float()
    }
}

pub fn platform_config_path() -> PathBuf {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(config_home).join("typio/platform.toml");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".config/typio/platform.toml");
    }
    Path::new("/tmp").join("typio-platform.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_preserves_comments_and_unknown_keys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("platform.toml");
        fs::write(
            &path,
            "# keep me\n[display]\npanel_theme = \"auto\"\nunknown = 42\n",
        )
        .unwrap();

        let mut config = PlatformConfig::load_from(path.clone()).unwrap();
        config.update(&DisplaySettings {
            theme: "dark",
            layout: "vertical",
            font_size: 13.0,
            font_family: "mono",
            panel_mode_indicator: true,
            anchor_probe: false,
            anchor_probe_timeout_ms: 275,
        });
        config.save().unwrap();

        let saved = fs::read_to_string(path).unwrap();
        assert!(saved.contains("# keep me"));
        assert!(saved.contains("unknown = 42"));
        assert!(saved.contains("panel_theme = \"dark\""));
        assert!(saved.contains("anchor_probe_timeout_ms = 275"));
    }

    #[test]
    fn missing_file_uses_documented_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let config = PlatformConfig::load_from(directory.path().join("platform.toml")).unwrap();
        assert_eq!(config.theme(), "auto");
        assert_eq!(config.candidate_layout(), "horizontal");
        assert_eq!(config.font_size(), 11.0);
        assert_eq!(config.font_family(), DEFAULT_FONT_FAMILY);
        assert!(config.anchor_probe());
    }

    #[test]
    fn unrecognised_font_family_reads_back_as_default() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("platform.toml");
        // A pre-existing free-text family name is not a valid family class.
        fs::write(&path, "[display]\nfont_family = \"Noto Sans CJK SC\"\n").unwrap();

        let config = PlatformConfig::load_from(path).unwrap();
        assert_eq!(config.font_family(), DEFAULT_FONT_FAMILY);
    }
}
