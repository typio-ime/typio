//! Main Typio runtime instance.

use crate::config::{Config, ConfigError, ConfigValue};
use crate::config_schema;
use crate::core::engine::{EngineError, EngineMode};
use crate::core::registry::{EngineRegistry, SwitchDirection};
use std::cell::{Ref, RefCell, RefMut};
use std::path::{Path, PathBuf};
use std::rc::Rc;

const CONFIG_FILE_NAME: &str = "core.toml";

type ModeObserver = Box<dyn Fn(&EngineMode) + 'static>;

pub(crate) struct RuntimeState {
    last_mode: Option<EngineMode>,
    mode_observer: Option<ModeObserver>,
}

impl RuntimeState {
    pub(crate) fn observe_mode(&mut self, mode: EngineMode, announce: bool) {
        self.last_mode = Some(mode);
        if announce && let (Some(observer), Some(mode)) = (&self.mode_observer, &self.last_mode) {
            observer(mode);
        }
    }
}

/// Core Typio runtime owned by the host main loop.
pub struct TypioInstance {
    pub(crate) registry: Rc<RefCell<EngineRegistry>>,
    pub(crate) runtime: Rc<RefCell<RuntimeState>>,
    config: Config,
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
    engine_dirs: Vec<PathBuf>,
    initialized: bool,
}

impl TypioInstance {
    /// Construct a runtime. Engine discovery remains a host concern.
    pub fn new_rust(
        config_dir: Option<&str>,
        data_dir: Option<&str>,
        state_dir: Option<&str>,
        engine_dirs: Vec<String>,
    ) -> Box<Self> {
        Box::new(Self {
            registry: Rc::new(RefCell::new(EngineRegistry::new())),
            runtime: Rc::new(RefCell::new(RuntimeState {
                last_mode: None,
                mode_observer: None,
            })),
            config: Config::new(),
            config_dir: config_dir
                .map(PathBuf::from)
                .unwrap_or_else(default_config_dir),
            data_dir: data_dir.map(PathBuf::from).unwrap_or_else(default_data_dir),
            state_dir: state_dir
                .map(PathBuf::from)
                .unwrap_or_else(default_state_dir),
            engine_dirs: engine_dirs
                .into_iter()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .collect(),
            initialized: false,
        })
    }

    /// Create directories, load configuration, and initialize persistent
    /// registry state. Idempotent.
    pub fn init_rust(&mut self) -> Result<(), ConfigError> {
        if self.initialized {
            return Ok(());
        }
        for directory in [&self.config_dir, &self.data_dir, &self.state_dir] {
            std::fs::create_dir_all(directory).map_err(ConfigError::Io)?;
        }
        let path = self.config_dir.join(CONFIG_FILE_NAME);
        self.config = if path.exists() {
            Config::load(&path)?
        } else {
            Config::new()
        };
        config_schema::apply_defaults(&mut self.config);
        let config_dir = self.config_dir.to_string_lossy().into_owned();
        let data_dir = self.data_dir.to_string_lossy().into_owned();
        let state_dir = self.state_dir.to_string_lossy().into_owned();
        let mut registry = self.registry.borrow_mut();
        registry.set_runtime_dirs(&config_dir, &data_dir, &state_dir);
        registry.set_state_dir(&state_dir);
        self.initialized = true;
        Ok(())
    }

    /// Persist configuration and mark the runtime stopped.
    pub fn shutdown_rust(&mut self) {
        if self.initialized {
            if let Err(error) = self.save_config_rust() {
                log::warn!("failed to save Typio configuration during shutdown: {error}");
            }
            self.initialized = false;
        }
    }

    /// Borrow the engine registry after initialization.
    pub fn registry_rust(&self) -> Option<Ref<'_, EngineRegistry>> {
        self.initialized.then(|| self.registry.borrow())
    }

    /// Mutably borrow the engine registry after initialization.
    pub fn registry_rust_mut(&self) -> Option<RefMut<'_, EngineRegistry>> {
        self.initialized.then(|| self.registry.borrow_mut())
    }

    /// Borrow the configuration after initialization.
    pub fn config_rust(&self) -> Option<&Config> {
        self.initialized.then_some(&self.config)
    }

    /// Mutably borrow the configuration after initialization.
    pub fn config_rust_mut(&mut self) -> Option<&mut Config> {
        self.initialized.then_some(&mut self.config)
    }

    /// Config directory used by this runtime.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Data directory used by this runtime.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// State directory used by this runtime.
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Engine-manifest directories retained for host-side reload.
    pub fn engine_dirs_rust(&self) -> impl Iterator<Item = &Path> {
        self.engine_dirs.iter().map(PathBuf::as_path)
    }

    /// Serialize current user configuration.
    pub fn config_text(&self) -> Option<String> {
        self.config_rust().map(Config::to_toml)
    }

    /// Persist current configuration atomically.
    pub fn save_config_rust(&self) -> Result<(), ConfigError> {
        self.config.save(&self.config_dir.join(CONFIG_FILE_NAME))
    }

    /// Reload configuration and notify active engines.
    pub fn reload_config_rust(&mut self) -> Result<(), ConfigError> {
        let path = self.config_dir.join(CONFIG_FILE_NAME);
        let mut replacement = if path.exists() {
            Config::load(&path)?
        } else {
            Config::new()
        };
        config_schema::apply_defaults(&mut replacement);
        self.config = replacement;
        self.registry.borrow_mut().reload_active_config();
        Ok(())
    }

    /// Last keyboard mode observed on a request/reply boundary.
    pub fn last_keyboard_mode(&self) -> Option<EngineMode> {
        self.runtime.borrow().last_mode.clone()
    }

    /// Enabled languages, falling back to registered engine declarations.
    pub fn enabled_languages(&self) -> Vec<String> {
        let configured = match self.config.value("languages.enabled") {
            Some(ConfigValue::Array(values)) => values
                .iter()
                .filter_map(|value| match value {
                    ConfigValue::String(value) if !value.trim().is_empty() => {
                        Some(value.trim().to_string())
                    }
                    _ => None,
                })
                .collect(),
            Some(ConfigValue::String(value)) => value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        };
        if configured.is_empty() {
            self.registry.borrow().known_languages()
        } else {
            configured
        }
    }

    /// Activate a language using configured modality overrides.
    pub fn activate_language(&self, tag: &str) -> crate::core::engine::Result<()> {
        let keyboard = self.language_override(tag, "keyboard");
        let voice = self.language_override(tag, "voice");
        self.registry
            .borrow_mut()
            .activate_language(tag, keyboard.as_deref(), voice.as_deref())
    }

    /// Activate a language with an explicit keyboard engine.
    pub fn activate_language_keyboard(
        &self,
        tag: &str,
        engine: &str,
    ) -> crate::core::engine::Result<()> {
        let voice = self.language_override(tag, "voice");
        self.registry
            .borrow_mut()
            .activate_language(tag, Some(engine), voice.as_deref())
    }

    /// Cycle configured languages.
    pub fn cycle_language(&self, direction: SwitchDirection) -> crate::core::engine::Result<()> {
        let enabled = self.enabled_languages();
        let target = self
            .registry
            .borrow()
            .cycle_language(&enabled, direction)
            .ok_or(EngineError::NotFound)?;
        self.activate_language(&target)
    }

    /// Restore the persisted language, or the first enabled language.
    pub fn restore_language(&self) -> crate::core::engine::Result<()> {
        let enabled = self.enabled_languages();
        if enabled.is_empty() {
            return Err(EngineError::NotFound);
        }
        let target = self
            .registry
            .borrow()
            .last_used_language()
            .filter(|target| {
                enabled
                    .iter()
                    .any(|enabled| enabled.eq_ignore_ascii_case(target))
            })
            .map(str::to_string)
            .unwrap_or_else(|| enabled[0].clone());
        self.activate_language(&target)
    }

    /// Observe deliberate keyboard-mode changes on the host main loop.
    pub fn set_mode_observer<F>(&self, observer: F)
    where
        F: Fn(&EngineMode) + 'static,
    {
        self.runtime.borrow_mut().mode_observer = Some(Box::new(observer));
    }

    fn language_override(&self, tag: &str, modality: &str) -> Option<String> {
        match self.config.value(&format!("languages.{tag}.{modality}")) {
            Some(ConfigValue::String(value)) => Some(value.clone()),
            _ => None,
        }
    }
}

impl Drop for TypioInstance {
    fn drop(&mut self) {
        self.shutdown_rust();
    }
}

fn xdg_path(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(fallback)
        })
}

fn default_config_dir() -> PathBuf {
    xdg_path("XDG_CONFIG_HOME", ".config").join("typio")
}

fn default_data_dir() -> PathBuf {
    xdg_path("XDG_DATA_HOME", ".local/share").join("typio")
}

fn default_state_dir() -> PathBuf {
    xdg_path("XDG_STATE_HOME", ".local/state").join("typio")
}
