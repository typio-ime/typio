//! Engine registry: lifecycle and switching (ADR-0005).

use crate::core::engine::backend::ProcessBackend;
use crate::core::engine::{EngineError, EngineType, InstanceHandle, Result};
use crate::log::log_msg;
use crate::types::TypioLogLevel;

use std::collections::BTreeMap;
use std::time::Instant;

/// Direction for keyboard switching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchDirection {
    /// Forward in the engine list.
    Next,
    /// Backward in the engine list.
    Previous,
}

/// A single engine slot in the registry.
pub struct EngineSlot {
    /// Engine name.
    pub name: String,
    /// Out-of-process engine backend.
    pub backend: ProcessBackend,
    /// Whether the engine is currently active.
    pub active: bool,
    /// Last time the engine was active.
    pub last_activity: Instant,
}

/// Registry of all engines and their activation state.
const ENGINE_STATE_FILE: &str = "engine-state.toml";

#[derive(Debug, Default, Clone)]
struct RecentEngines {
    primary: Option<String>,
    secondary: Option<String>,
}

/// Engine registry managing all loaded engines.
pub struct EngineRegistry {
    slots: Vec<EngineSlot>,
    active_keyboard: Option<usize>,
    active_voice: Option<usize>,
    instance: InstanceHandle,
    recent_keyboard: RecentEngines,
    recent_voice: RecentEngines,
    active_language: Option<String>,
    recent_language: Option<String>,
    /// Last keyboard engine chosen per language tag (ADR-0018). Drives
    /// "switch to language X → reuse the engine you last used for X" instead
    /// of always falling back to registration order. Keyboard-only: the
    /// active language tracks the text-input (keyboard) modality, so a voice
    /// engine picked independently must not populate this map.
    last_engine_per_language: BTreeMap<String, String>,
    state_dir: Option<String>,
}

impl EngineRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            slots: Vec::with_capacity(8),
            active_keyboard: None,
            active_voice: None,
            instance: InstanceHandle::new(),
            recent_keyboard: RecentEngines::default(),
            recent_voice: RecentEngines::default(),
            active_language: None,
            recent_language: None,
            last_engine_per_language: BTreeMap::new(),
            state_dir: None,
        }
    }

    /// Set the directory used to persist engine state.
    pub fn set_state_dir(&mut self, dir: &str) {
        self.state_dir = Some(dir.to_string());
        self.load_state();
    }

    fn load_state(&mut self) {
        let dir = match self.state_dir.as_ref() {
            Some(d) => d,
            None => return,
        };
        let path = std::path::Path::new(dir).join(ENGINE_STATE_FILE);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(_) => return,
        };
        if let Some(table) = parsed.get("keyboard").and_then(|v| v.as_table()) {
            Self::load_recent_table(table, &mut self.recent_keyboard);
        }
        if let Some(table) = parsed.get("voice").and_then(|v| v.as_table()) {
            Self::load_recent_table(table, &mut self.recent_voice);
        }
        if let Some(active) = parsed
            .get("language")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("active"))
            .and_then(|v| v.as_str())
        {
            self.recent_language = Some(active.to_string());
        }
        if let Some(table) = parsed.get("language-engines").and_then(|v| v.as_table()) {
            for (lang, val) in table {
                if let Some(name) = val.as_str() {
                    self.last_engine_per_language
                        .insert(lang.clone(), name.to_string());
                }
            }
        }
    }

    fn load_recent_table(table: &toml::map::Map<String, toml::Value>, recent: &mut RecentEngines) {
        if let Some(primary) = table.get("primary").and_then(|v| v.as_str()) {
            recent.primary = Some(primary.to_string());
        }
        if let Some(secondary) = table.get("secondary").and_then(|v| v.as_str()) {
            recent.secondary = Some(secondary.to_string());
        }
    }

    fn save_state(&self) {
        let dir = match self.state_dir.as_ref() {
            Some(d) => d,
            None => return,
        };
        let path = std::path::Path::new(dir).join(ENGINE_STATE_FILE);
        let mut lines = vec![
            "# Typio engine state (TOML-compatible subset)".to_string(),
            "".to_string(),
        ];
        Self::append_recent_section(&mut lines, "keyboard", &self.recent_keyboard);
        Self::append_recent_section(&mut lines, "voice", &self.recent_voice);
        if let Some(ref active) = self.active_language {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[language]".to_string());
            lines.push(format!(r#"active = "{}""#, active));
        }
        if !self.last_engine_per_language.is_empty() {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[language-engines]".to_string());
            for (lang, name) in &self.last_engine_per_language {
                lines.push(format!(r#""{}" = "{}""#, lang, name));
            }
        }
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, lines.join("\n") + "\n");
    }

    fn append_recent_section(lines: &mut Vec<String>, section: &str, recent: &RecentEngines) {
        if recent.primary.is_none() && recent.secondary.is_none() {
            return;
        }
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{}]", section));
        if let Some(ref primary) = recent.primary {
            lines.push(format!(r#"primary = "{}""#, primary));
        }
        if let Some(ref secondary) = recent.secondary {
            lines.push(format!(r#"secondary = "{}""#, secondary));
        }
    }

    /// Activate the most recently used keyboard engine, falling back to the
    /// first available keyboard engine if no state exists.
    pub fn activate_last_used_keyboard(&mut self) -> Result<()> {
        let target = self.recent_target(&self.recent_keyboard, EngineType::Keyboard);
        match target {
            Some(name) => self.activate_keyboard(&name),
            None => Err(EngineError::NotFound),
        }
    }

    /// Activate the most recently used voice engine, falling back to the
    /// first available voice engine if no state exists.
    pub fn activate_last_used_voice(&mut self) -> Result<()> {
        let target = self.recent_target(&self.recent_voice, EngineType::Voice);
        match target {
            Some(name) => self.activate_voice(&name),
            None => Err(EngineError::NotFound),
        }
    }

    /* --------------------------------------------------------------------- */
    /* Registration                                                          */
    /* --------------------------------------------------------------------- */

    /// Register a new out-of-process engine backend.
    pub fn register(&mut self, backend: ProcessBackend) -> Result<()> {
        let name = backend.info().name.clone();
        if self.find_index(&name).is_some() {
            return Err(EngineError::AlreadyExists);
        }
        self.slots.push(EngineSlot {
            name,
            backend,
            active: false,
            last_activity: Instant::now(),
        });
        Ok(())
    }

    /// Unregister an engine by name.
    ///
    /// If the engine is currently active, it is deactivated first.
    pub fn unregister(&mut self, name: &str) -> Result<()> {
        let idx = self.find_index(name).ok_or(EngineError::NotFound)?;

        if Some(idx) == self.active_keyboard {
            self.deactivate_slot(idx)?;
            self.active_keyboard = None;
        }
        if Some(idx) == self.active_voice {
            self.deactivate_slot(idx)?;
            self.active_voice = None;
        }

        // Adjust indices after removal.
        self.adjust_indices_after_removal(idx);

        // Drop any per-language memory pointing at the removed engine; the
        // next language switch will fall back to registration order.
        self.last_engine_per_language.retain(|_, v| v != name);

        let mut slot = self.slots.remove(idx);
        slot.backend.destroy();
        Ok(())
    }

    /* --------------------------------------------------------------------- */
    /* Queries                                                               */
    /* --------------------------------------------------------------------- */

    /// Return the index of the named engine, if registered.
    pub fn find_index(&self, name: &str) -> Option<usize> {
        self.slots.iter().position(|s| s.name == name)
    }

    /// Return a reference to the named engine slot, if registered.
    pub fn find_slot(&self, name: &str) -> Option<&EngineSlot> {
        self.find_index(name).map(|i| &self.slots[i])
    }

    /// Return the metadata for a registered engine by name.
    pub fn engine_info(&self, name: &str) -> Option<&crate::core::engine::EngineInfo> {
        self.find_slot(name).map(|slot| slot.backend.info())
    }

    /// List all registered keyboard engine names.
    pub fn list_keyboards(&self) -> Vec<&str> {
        self.slots
            .iter()
            .filter(|s| s.backend.info().engine_type == EngineType::Keyboard)
            .map(|s| s.name.as_str())
            .collect()
    }

    /// List all registered voice engine names.
    pub fn list_voices(&self) -> Vec<&str> {
        self.slots
            .iter()
            .filter(|s| s.backend.info().engine_type == EngineType::Voice)
            .map(|s| s.name.as_str())
            .collect()
    }

    /// Return the name of the currently active keyboard engine, if any.
    pub fn active_keyboard_name(&self) -> Option<&str> {
        self.active_keyboard
            .and_then(|i| self.slots.get(i))
            .map(|s| s.name.as_str())
    }

    /// Return the name of the currently active voice engine, if any.
    pub fn active_voice_name(&self) -> Option<&str> {
        self.active_voice
            .and_then(|i| self.slots.get(i))
            .map(|s| s.name.as_str())
    }

    /// Return the current availability of the active keyboard engine.
    pub fn active_keyboard_availability(&self) -> crate::core::engine::EngineAvailability {
        if let Some(idx) = self.active_keyboard {
            let slot = &self.slots[idx];
            slot.backend
                .with_engine_ref(|engine| engine.availability())
                .unwrap_or(crate::core::engine::EngineAvailability::Failed)
        } else {
            crate::core::engine::EngineAvailability::Failed
        }
    }

    /// Return the current availability of the active voice engine.
    pub fn active_voice_availability(&self) -> crate::core::engine::EngineAvailability {
        if let Some(idx) = self.active_voice {
            let slot = &self.slots[idx];
            slot.backend
                .with_engine_ref(|engine| engine.availability())
                .unwrap_or(crate::core::engine::EngineAvailability::Failed)
        } else {
            crate::core::engine::EngineAvailability::Failed
        }
    }

    /* --------------------------------------------------------------------- */
    /* Activation / Switching                                                */
    /* --------------------------------------------------------------------- */

    /// Activate a keyboard engine by name.
    pub fn activate_keyboard(&mut self, name: &str) -> Result<()> {
        let idx = self.find_index(name).ok_or(EngineError::NotFound)?;

        if self.slots[idx].backend.info().engine_type != EngineType::Keyboard {
            return Err(EngineError::InvalidArgument);
        }

        // Deactivate current keyboard if any.
        if let Some(current) = self.active_keyboard {
            if current == idx {
                return Ok(());
            }
            self.deactivate_slot(current)?;
        }

        self.activate_slot(idx)?;
        self.active_keyboard = Some(idx);
        Self::update_recent_pair(&mut self.recent_keyboard, name);
        self.reconcile_active_language(idx);
        self.record_engine_languages(idx);
        self.save_state();
        Ok(())
    }

    /// Deactivate the currently active keyboard engine.
    pub fn deactivate_current_keyboard(&mut self) -> Result<()> {
        if let Some(idx) = self.active_keyboard.take() {
            self.deactivate_slot(idx)?;
        }
        Ok(())
    }

    /// Switch to the next or previous keyboard engine.
    pub fn switch_keyboard(&mut self, direction: SwitchDirection) -> Result<()> {
        let keyboards: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.backend.info().engine_type == EngineType::Keyboard)
            .map(|(i, _)| i)
            .collect();

        if keyboards.is_empty() {
            return Err(EngineError::NotFound);
        }

        let current = self
            .active_keyboard
            .and_then(|a| keyboards.iter().position(|&k| k == a))
            .unwrap_or(0);

        let next = match direction {
            SwitchDirection::Next => (current + 1) % keyboards.len(),
            SwitchDirection::Previous => {
                if current == 0 {
                    keyboards.len() - 1
                } else {
                    current - 1
                }
            }
        };

        let target = keyboards[next];
        let name = self.slots[target].name.clone();
        self.activate_keyboard(&name)
    }

    /* --------------------------------------------------------------------- */
    /* Voice activation / switching                                          */
    /* --------------------------------------------------------------------- */

    /// Activate a voice engine by name.
    pub fn activate_voice(&mut self, name: &str) -> Result<()> {
        let idx = self.find_index(name).ok_or(EngineError::NotFound)?;
        if self.slots[idx].backend.info().engine_type != EngineType::Voice {
            return Err(EngineError::InvalidArgument);
        }
        if let Some(current) = self.active_voice {
            if current == idx {
                return Ok(());
            }
            self.deactivate_slot(current)?;
        }
        self.activate_slot(idx)?;
        self.active_voice = Some(idx);
        Self::update_recent_pair(&mut self.recent_voice, name);
        self.save_state();
        Ok(())
    }

    /// Deactivate the currently active voice engine.
    pub fn deactivate_current_voice(&mut self) -> Result<()> {
        if let Some(idx) = self.active_voice.take() {
            self.deactivate_slot(idx)?;
        }
        Ok(())
    }

    /// Switch to the next or previous voice engine.
    pub fn switch_voice(&mut self, direction: SwitchDirection) -> Result<()> {
        let voices: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.backend.info().engine_type == EngineType::Voice)
            .map(|(i, _)| i)
            .collect();

        if voices.is_empty() {
            return Err(EngineError::NotFound);
        }

        let current = self
            .active_voice
            .and_then(|a| voices.iter().position(|&k| k == a))
            .unwrap_or(0);

        let next = match direction {
            SwitchDirection::Next => (current + 1) % voices.len(),
            SwitchDirection::Previous => {
                if current == 0 {
                    voices.len() - 1
                } else {
                    current - 1
                }
            }
        };

        let target = voices[next];
        let name = self.slots[target].name.clone();
        self.activate_voice(&name)
    }

    /* --------------------------------------------------------------------- */
    /* Language model (ADR-0018)                                             */
    /* --------------------------------------------------------------------- */

    /// Replace the declared language list of a registered engine.
    ///
    /// The first entry becomes the primary `language`. Hosts call this right
    /// after registration with the manifest's `languages` value.
    pub fn set_engine_languages(&mut self, name: &str, languages: Vec<String>) -> Result<()> {
        let idx = self.find_index(name).ok_or(EngineError::NotFound)?;
        let info = self.slots[idx].backend.info_mut();
        if let Some(first) = languages.first() {
            info.language = first.clone();
        }
        info.languages = languages;
        Ok(())
    }

    /// Languages declared by at least one registered engine, deduplicated in
    /// registration order. The pseudo-tags `und` and `mul` are excluded.
    pub fn known_languages(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for slot in &self.slots {
            for lang in slot.backend.info().effective_languages() {
                if Self::is_pseudo_tag(lang) {
                    continue;
                }
                if !out.iter().any(|l| l.eq_ignore_ascii_case(lang)) {
                    out.push(lang.clone());
                }
            }
        }
        out
    }

    /// Pseudo language tags that carry no concrete language: `und`
    /// (undetermined) and `mul` (multiple/all). Used to keep both
    /// `known_languages` and language reconciliation consistent.
    fn is_pseudo_tag(tag: &str) -> bool {
        tag.eq_ignore_ascii_case("und") || tag.eq_ignore_ascii_case("mul")
    }

    /// The currently active language tag, if a language was activated.
    pub fn active_language(&self) -> Option<&str> {
        self.active_language.as_deref()
    }

    /// The language persisted by the last `activate_language` across runs.
    pub fn last_used_language(&self) -> Option<&str> {
        self.recent_language.as_deref()
    }

    /// Compute the language that a Next/Previous step from the active
    /// language lands on within `enabled`. Returns `None` when `enabled`
    /// has fewer than two entries — a single (or empty) language list has
    /// nothing to cycle, so hosts can fall back to engine cycling. When the
    /// active language is not in `enabled`, Next and Previous both land on
    /// the first entry.
    pub fn cycle_language(&self, enabled: &[String], direction: SwitchDirection) -> Option<String> {
        if enabled.len() < 2 {
            return None;
        }
        let pos = self
            .active_language
            .as_deref()
            .and_then(|cur| enabled.iter().position(|e| e.eq_ignore_ascii_case(cur)));
        let next = match pos {
            None => 0,
            Some(p) => match direction {
                SwitchDirection::Next => (p + 1) % enabled.len(),
                SwitchDirection::Previous => (p + enabled.len() - 1) % enabled.len(),
            },
        };
        Some(enabled[next].clone())
    }

    /// Activate a language: re-resolve and retarget every modality slot.
    ///
    /// Per modality the engine is chosen by `override_` first (`"none"`
    /// forces an empty slot; an unregistered override falls back), then the
    /// first registered engine declaring a matching language. A modality
    /// with no engine is deactivated — for keyboards this yields raw
    /// passthrough, which is the contract for layout-only languages.
    ///
    /// Engine activation is best-effort: a failing engine deactivates its
    /// slot instead of aborting the language switch.
    pub fn activate_language(
        &mut self,
        tag: &str,
        keyboard_override: Option<&str>,
        voice_override: Option<&str>,
    ) -> Result<()> {
        if tag.is_empty() {
            return Err(EngineError::InvalidArgument);
        }
        self.retarget_modality(tag, EngineType::Keyboard, keyboard_override);
        self.retarget_modality(tag, EngineType::Voice, voice_override);
        self.active_language = Some(tag.to_string());
        self.save_state();
        Ok(())
    }

    /// Reconcile the active language with the keyboard engine just activated
    /// at `idx`.
    ///
    /// Switching keyboards directly (`activate_keyboard`, e.g. via the tray
    /// submenu, `typioctl keyboard use`, or hotkeys) does not flow through
    /// `activate_language`, so the recorded active language would otherwise
    /// stay stale — leaving the indicator/tray badge showing the previous
    /// language instead of the newly active keyboard's language.
    ///
    /// If the engine declares a concrete primary language (i.e. not `und`/
    /// `mul`) and the current active language is absent or not among the
    /// engine's declared languages, the active language is retargeted to the
    /// engine's primary. Switching between keyboards of the same language is
    /// a no-op (no churn), and engines that only declare pseudo-tags leave
    /// the active language untouched. Voice-engine activation is intentionally
    /// excluded: the badge reflects the keyboard (text-input) language, so an
    /// independently chosen voice engine must not move it.
    fn reconcile_active_language(&mut self, idx: usize) {
        let (primary, declared): (Option<String>, Vec<String>) = {
            let info = self.slots[idx].backend.info();
            let effective = info.effective_languages();
            let primary = effective
                .first()
                .filter(|p| !Self::is_pseudo_tag(p))
                .cloned();
            (primary, effective.to_vec())
        };
        let Some(primary) = primary else {
            return;
        };
        let current_matches = self
            .active_language
            .as_deref()
            .is_some_and(|cur| declared.iter().any(|d| Self::language_matches(d, cur)));
        if !current_matches {
            self.active_language = Some(primary);
        }
    }

    /// Record the keyboard engine at `idx` as the last used for every concrete
    /// language it declares, so a later switch to one of those languages
    /// reuses it instead of registration order.
    fn record_engine_languages(&mut self, idx: usize) {
        let name = self.slots[idx].name.clone();
        let langs = self.slots[idx]
            .backend
            .info()
            .effective_languages()
            .to_vec();
        for lang in &langs {
            if Self::is_pseudo_tag(lang) {
                continue;
            }
            self.last_engine_per_language
                .insert(lang.clone(), name.clone());
        }
    }

    /// Return the keyboard engine the user last used for `tag`, if it is still
    /// registered. BCP-47 matching lets a memory keyed on `zh-Hans` also
    /// satisfy a `zh` request (and vice versa). `None` lets the caller
    /// continue to registration-order resolution.
    fn remembered_keyboard_for(&self, tag: &str) -> Option<String> {
        for (lang, name) in &self.last_engine_per_language {
            if Self::language_matches(lang, tag)
                && self
                    .find_index_of_type(name, EngineType::Keyboard)
                    .is_some()
            {
                return Some(name.clone());
            }
        }
        None
    }

    fn retarget_modality(&mut self, tag: &str, engine_type: EngineType, override_: Option<&str>) {
        let target = self.resolve_language_engine(tag, engine_type, override_);
        let result = match (&target, engine_type) {
            (Some(name), EngineType::Keyboard) => self.activate_keyboard(name),
            (Some(name), EngineType::Voice) => self.activate_voice(name),
            (None, EngineType::Keyboard) => self.deactivate_current_keyboard(),
            (None, EngineType::Voice) => self.deactivate_current_voice(),
        };
        if let Err(e) = result {
            log_msg(
                TypioLogLevel::TypioLogWarning,
                &format!(
                    "Language '{}': engine '{}' activation failed ({:?}); slot deactivated",
                    tag,
                    target.as_deref().unwrap_or("none"),
                    e
                ),
            );
            let _ = match engine_type {
                EngineType::Keyboard => self.deactivate_current_keyboard(),
                EngineType::Voice => self.deactivate_current_voice(),
            };
        }
    }

    /// Pick the engine that serves `tag` for one modality, or `None` for an
    /// empty slot.
    ///
    /// Resolution order: an explicit `override_` first (`"none"` forces an
    /// empty slot; an unregistered override falls back), then the keyboard
    /// engine the user last used for this language, then the first
    /// registered engine declaring the language (registration order).
    pub fn resolve_language_engine(
        &self,
        tag: &str,
        engine_type: EngineType,
        override_: Option<&str>,
    ) -> Option<String> {
        if let Some(o) = override_.map(str::trim) {
            if o.eq_ignore_ascii_case("none") {
                return None;
            }
            if !o.is_empty() {
                if self.find_index_of_type(o, engine_type).is_some() {
                    return Some(o.to_string());
                }
                log_msg(
                    TypioLogLevel::TypioLogWarning,
                    &format!(
                        "Language '{}': configured engine '{}' is not registered; \
                         falling back to declared languages",
                        tag, o
                    ),
                );
            }
        }
        if engine_type == EngineType::Keyboard {
            if let Some(remembered) = self.remembered_keyboard_for(tag) {
                return Some(remembered);
            }
        }
        self.slots
            .iter()
            .filter(|s| s.backend.info().engine_type == engine_type)
            .find(|s| {
                s.backend
                    .info()
                    .effective_languages()
                    .iter()
                    .any(|d| Self::language_matches(d, tag))
            })
            .map(|s| s.name.clone())
    }

    /// BCP-47 matching used for language→engine resolution: case-insensitive
    /// equality, the `mul` wildcard, or a primary-subtag prefix at a `-`
    /// boundary in either direction (`zh` serves `zh-Hans` and vice versa).
    fn language_matches(declared: &str, tag: &str) -> bool {
        if declared.eq_ignore_ascii_case("mul") {
            return true;
        }
        if declared.eq_ignore_ascii_case(tag) {
            return true;
        }
        let (short, long) = if declared.len() < tag.len() {
            (declared.as_bytes(), tag.as_bytes())
        } else {
            (tag.as_bytes(), declared.as_bytes())
        };
        long.len() > short.len()
            && long[..short.len()].eq_ignore_ascii_case(short)
            && long[short.len()] == b'-'
    }

    /* --------------------------------------------------------------------- */
    /* Commit tracking (recent pair)                                         */
    /* --------------------------------------------------------------------- */

    /// Notify that the active keyboard committed text.
    ///
    /// Updates the keyboard recent-primary/recent-secondary pair used for
    /// fast toggling between two keyboard engines.
    pub fn notify_keyboard_commit(&mut self) {
        if let Some(idx) = self.active_keyboard {
            let name = self.slots[idx].name.clone();
            Self::update_recent_pair(&mut self.recent_keyboard, &name);
            self.save_state();
        }
    }

    /// Notify that the active voice committed text.
    pub fn notify_voice_commit(&mut self) {
        if let Some(idx) = self.active_voice {
            let name = self.slots[idx].name.clone();
            Self::update_recent_pair(&mut self.recent_voice, &name);
            self.save_state();
        }
    }

    fn update_recent_pair(recent: &mut RecentEngines, name: &str) {
        if name.is_empty() {
            return;
        }
        if recent.primary.as_deref() == Some(name) {
            return;
        }
        recent.secondary = recent.primary.clone();
        recent.primary = Some(name.to_string());
    }

    /* --------------------------------------------------------------------- */
    /* Direct engine operations (for input_context / voice integration)      */
    /* --------------------------------------------------------------------- */

    /// Invoke a command on the active keyboard engine.
    ///
    /// Returns `NotFound` if no keyboard is active, `NotSupported` if the
    /// engine does not expose this command.
    pub fn invoke_active_keyboard_command(&mut self, id: &str) -> Result<()> {
        match self.active_keyboard {
            Some(idx) => {
                let name = self.slots[idx].name.clone();
                self.invoke_command(&name, id)
            }
            None => Err(EngineError::NotFound),
        }
    }

    /// Invoke a command on a named engine (any kind) (ADR-0008).
    pub fn invoke_command(&mut self, engine_name: &str, id: &str) -> Result<()> {
        let idx = self.find_index(engine_name).ok_or(EngineError::NotFound)?;
        let slot = &mut self.slots[idx];
        match slot.backend.with_engine(|engine| engine.invoke_command(id)) {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => Err(e),
            None => Err(EngineError::NotSupported),
        }
    }

    /// Return the commands exposed by a named engine (ADR-0008).
    ///
    /// Owned `Vec` (already translated out of the engine-owned transient
    /// array); callers can hold it across other registry calls.
    pub fn list_commands(
        &mut self,
        engine_name: &str,
    ) -> Result<Vec<crate::core::engine::Command>> {
        let idx = self.find_index(engine_name).ok_or(EngineError::NotFound)?;
        let slot = &mut self.slots[idx];
        match slot.backend.with_engine(|engine| engine.list_commands()) {
            Some(v) => Ok(v),
            None => Ok(vec![]),
        }
    }

    /// Notify a named engine that one of its config keys changed (ADR-0008).
    ///
    /// Used by the host after writing `engines.<name>.<key>` through the
    /// unified config tree. No-op if the engine does not implement
    /// `on_config_change`.
    pub fn notify_config_change(
        &mut self,
        engine_name: &str,
        key: &str,
        value: &str,
    ) -> Result<()> {
        let idx = self.find_index(engine_name).ok_or(EngineError::NotFound)?;
        let slot = &mut self.slots[idx];
        slot.backend
            .with_engine(|engine| engine.on_config_change(key, value));
        Ok(())
    }

    /// Reload config on the active keyboard engine.
    ///
    /// Called when the user edits `core.toml` or sends a reload signal.
    /// Engine-side failures are logged but do not propagate — a misconfigured
    /// engine should not crash the host.
    pub fn reload_active_config(&mut self) {
        for idx in [self.active_keyboard].into_iter().flatten() {
            let slot = &mut self.slots[idx];
            let name = slot.name.clone();
            let r = slot.backend.with_engine(|engine| engine.reload_config());
            if let Some(Err(e)) = r {
                log_msg(
                    TypioLogLevel::TypioLogWarning,
                    &format!("Engine '{}' reload_config failed: {:?}", name, e),
                );
            }
        }
    }

    /// Forward `focus_in` to the active keyboard engine.
    pub fn focus_in_active_keyboard(&mut self, ctx: &mut crate::core::engine::InputContext) {
        if let Some(idx) = self.active_keyboard {
            let slot = &mut self.slots[idx];
            slot.backend.with_engine(|engine| engine.focus_in(ctx));
        }
    }

    /// Forward `focus_out` to the active keyboard engine.
    pub fn focus_out_active_keyboard(&mut self, ctx: &mut crate::core::engine::InputContext) {
        if let Some(idx) = self.active_keyboard {
            let slot = &mut self.slots[idx];
            slot.backend.with_engine(|engine| engine.focus_out(ctx));
        }
    }

    /// Forward `reset` to the active keyboard engine.
    pub fn reset_active_keyboard(&mut self, ctx: &mut crate::core::engine::InputContext) {
        if let Some(idx) = self.active_keyboard {
            let slot = &mut self.slots[idx];
            slot.backend.with_engine(|engine| engine.reset(ctx));
        }
    }

    /// Forward a key event to the active keyboard engine.
    pub fn process_key_active_keyboard(
        &mut self,
        ctx: &mut crate::core::engine::InputContext,
        event: &crate::core::engine::KeyEvent,
    ) -> crate::core::engine::KeyProcessResult {
        if let Some(idx) = self.active_keyboard {
            let slot = &mut self.slots[idx];
            slot.backend
                .with_engine(|engine| match engine.as_keyboard() {
                    Some(kb) => kb.process_key(ctx, event),
                    None => crate::core::engine::KeyProcessResult::NotHandled,
                })
                .unwrap_or(crate::core::engine::KeyProcessResult::NotHandled)
        } else {
            crate::core::engine::KeyProcessResult::NotHandled
        }
    }

    /// Restore/set the active keyboard engine's mode for `ctx`.
    ///
    /// The host calls this to re-apply a remembered mode on focus.
    /// Returns `NotSupported` when there is no active keyboard or the engine
    /// does not implement `set_active_mode`.
    pub fn set_active_mode_keyboard(
        &mut self,
        ctx: &mut crate::core::engine::InputContext,
        mode_id: &str,
    ) -> crate::core::engine::Result<()> {
        if let Some(idx) = self.active_keyboard {
            let slot = &mut self.slots[idx];
            slot.backend
                .with_engine(|engine| match engine.as_keyboard() {
                    Some(kb) => kb.set_active_mode(ctx, Some(mode_id)),
                    None => Err(crate::core::engine::EngineError::NotSupported),
                })
                .unwrap_or(Err(crate::core::engine::EngineError::NotSupported))
        } else {
            Err(crate::core::engine::EngineError::NotSupported)
        }
    }

    /// Drain a pending active-mode change from the active keyboard engine.
    ///
    /// Returns the new active mode once, the first time it is queried after a
    /// request changed it (see [`KeyboardEngine::take_changed_mode`]). The
    /// framework calls this after every mode-affecting request and turns a
    /// `Some` into a host notification.
    pub fn take_active_keyboard_changed_mode(&mut self) -> Option<crate::core::engine::EngineMode> {
        let idx = self.active_keyboard?;
        let slot = &mut self.slots[idx];
        slot.backend
            .with_engine(|engine| engine.as_keyboard().and_then(|kb| kb.take_changed_mode()))
            .flatten()
    }

    /// Commit a candidate on the active keyboard engine.
    ///
    /// Returns `NotSupported` when there is no active keyboard or the engine
    /// does not implement `commit_candidate`.
    pub fn commit_candidate_active_keyboard(
        &mut self,
        ctx: &mut crate::core::engine::InputContext,
        candidate_index: i32,
    ) -> crate::core::engine::Result<()> {
        if let Some(idx) = self.active_keyboard {
            let slot = &mut self.slots[idx];
            slot.backend
                .with_engine(|engine| match engine.as_keyboard() {
                    Some(kb) => kb.commit_candidate(ctx, candidate_index),
                    None => Err(crate::core::engine::EngineError::NotSupported),
                })
                .unwrap_or(Err(crate::core::engine::EngineError::NotSupported))
        } else {
            Err(crate::core::engine::EngineError::NotSupported)
        }
    }

    /// Access the active voice engine immutably.
    pub fn with_active_voice<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&dyn crate::core::engine::VoiceEngine) -> R,
    {
        if let Some(idx) = self.active_voice {
            let slot = &self.slots[idx];
            slot.backend
                .with_engine_ref(|engine| engine.as_voice_ref().map(f))
                .flatten()
        } else {
            None
        }
    }

    /// Access the active voice engine mutably.
    pub fn with_active_voice_mut<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn crate::core::engine::VoiceEngine) -> R,
    {
        if let Some(idx) = self.active_voice {
            let slot = &mut self.slots[idx];
            slot.backend
                .with_engine(|engine| engine.as_voice().map(f))
                .flatten()
        } else {
            None
        }
    }

    /// Snapshot the active voice worker for an asynchronous inference job.
    ///
    /// The handle owns shared worker state and therefore remains valid if the
    /// main thread switches or unloads the registry slot while inference is
    /// running.
    pub(crate) fn snapshot_active_voice(
        &mut self,
    ) -> Option<crate::core::engine::backend::process::VoiceProcessHandle> {
        let idx = self.active_voice?;
        self.slots[idx].backend.voice_handle()
    }

    /// Query keyboard availability after recovering a poisoned worker.
    pub(crate) fn recovering_active_keyboard_availability(
        &mut self,
    ) -> crate::core::engine::EngineAvailability {
        let Some(idx) = self.active_keyboard else {
            return crate::core::engine::EngineAvailability::Failed;
        };
        self.slots[idx]
            .backend
            .with_engine(|engine| engine.availability())
            .unwrap_or(crate::core::engine::EngineAvailability::Failed)
    }

    /// Query voice availability after recovering a poisoned worker.
    pub(crate) fn recovering_active_voice_availability(
        &mut self,
    ) -> crate::core::engine::EngineAvailability {
        let Some(idx) = self.active_voice else {
            return crate::core::engine::EngineAvailability::Failed;
        };
        self.slots[idx]
            .backend
            .with_engine(|engine| engine.availability())
            .unwrap_or(crate::core::engine::EngineAvailability::Failed)
    }

    /// Return true if the active voice engine exists and reports ready.
    pub fn active_voice_is_ready(&self) -> bool {
        self.active_voice_availability() == crate::core::engine::EngineAvailability::Ready
    }

    /// Return true after recovering the active voice worker when necessary.
    pub(crate) fn recovering_active_voice_is_ready(&mut self) -> bool {
        self.recovering_active_voice_availability()
            == crate::core::engine::EngineAvailability::Ready
    }

    /* --------------------------------------------------------------------- */
    /* Internal helpers                                                      */
    /* --------------------------------------------------------------------- */

    fn activate_slot(&mut self, idx: usize) -> Result<()> {
        let slot = &mut self.slots[idx];
        if !slot.backend.is_instantiated() {
            if let Err(e) = slot.backend.instantiate() {
                log_msg(
                    TypioLogLevel::TypioLogError,
                    &format!("Engine '{}' instantiate failed: {:?}", slot.name, e),
                );
                return Err(e);
            }
        }
        let init_result = slot
            .backend
            .with_engine(|engine| engine.init(&mut self.instance))
            .unwrap_or(Err(EngineError::NotFound));
        if let Err(e) = init_result {
            log_msg(
                TypioLogLevel::TypioLogError,
                &format!("Engine '{}' init failed: {:?}", slot.name, e),
            );
            return Err(e);
        }
        slot.active = true;
        slot.last_activity = Instant::now();
        Ok(())
    }

    fn deactivate_slot(&mut self, idx: usize) -> Result<()> {
        let slot = &mut self.slots[idx];
        slot.backend.with_engine(|engine| engine.deactivate());
        slot.active = false;
        slot.last_activity = Instant::now();
        Ok(())
    }

    fn adjust_indices_after_removal(&mut self, removed: usize) {
        self.active_keyboard = self.active_keyboard.and_then(|i| {
            if i == removed {
                None
            } else if i > removed {
                Some(i - 1)
            } else {
                Some(i)
            }
        });
        self.active_voice = self.active_voice.and_then(|i| {
            if i == removed {
                None
            } else if i > removed {
                Some(i - 1)
            } else {
                Some(i)
            }
        });
    }

    fn recent_target(&self, recent: &RecentEngines, engine_type: EngineType) -> Option<String> {
        for name in [&recent.primary, &recent.secondary].into_iter().flatten() {
            if self.find_index_of_type(name, engine_type).is_some() {
                return Some(name.clone());
            }
        }
        self.slots
            .iter()
            .find(|slot| slot.backend.info().engine_type == engine_type)
            .map(|slot| slot.name.clone())
    }

    fn find_index_of_type(&self, name: &str, engine_type: EngineType) -> Option<usize> {
        self.find_index(name)
            .filter(|idx| self.slots[*idx].backend.info().engine_type == engine_type)
    }
}

impl Default for EngineRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::engine::backend::process::ProcessBackend;
    use crate::core::engine::{EngineInfo, EngineType};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn worker_script() -> String {
        let path: PathBuf = std::env::temp_dir().join(format!(
            "typio-libtypio-registry-test-{}-{}.py",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        fs::write(
            &path,
            r#"#!/usr/bin/env python3
import os
import struct
import sys

MAGIC = 0x54594550
MAJOR = 1
MINOR = 0
ENGINE_HELLO = 1
HOST_HELLO = 2
REQUEST = 3
RESPONSE = 4

name = sys.argv[1]
engine_type = sys.argv[2]
fd = int(os.environ.get("TYPIO_ENGINE_FD", "3"))

def read_exact(n):
    data = b""
    while len(data) < n:
        chunk = os.read(fd, n - len(data))
        if not chunk:
            raise SystemExit(0)
        data += chunk
    return data

def read_frame():
    header = read_exact(28)
    magic, major, minor, msg_type, flags, request_id, payload_len = struct.unpack("!IHHIIQI", header)
    if magic != MAGIC or major != MAJOR:
        raise SystemExit(2)
    return msg_type, request_id, read_exact(payload_len)

def write_frame(msg_type, request_id, payload):
    os.write(fd, struct.pack("!IHHIIQI", MAGIC, MAJOR, MINOR, msg_type, 0, request_id, len(payload)) + payload)

write_frame(ENGINE_HELLO, 0, f"protocol\t1.0\nengine\t{name}\ntype\t{engine_type}".encode())
msg_type, request_id, payload = read_frame()
if msg_type != HOST_HELLO:
    raise SystemExit(2)

while True:
    msg_type, request_id, payload = read_frame()
    if msg_type != REQUEST:
        continue
    line = payload.decode()
    if line == "shutdown":
        raise SystemExit(0)
    if line == "availability":
        response = "AVAILABILITY\tREADY\n"
    elif line.startswith("process-key"):
        response = "RESULT\tNOT_HANDLED\n"
    elif line.startswith("process-audio"):
        response = ""
    else:
        response = "OK\n"
    write_frame(RESPONSE, request_id, response.encode())
"#,
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn register_keyboard(reg: &mut EngineRegistry, name: &str) {
        let worker = worker_script();
        let backend = ProcessBackend::new(
            EngineInfo::new(name, EngineType::Keyboard),
            vec![worker, name.to_string(), "keyboard".to_string()],
        );
        reg.register(backend).unwrap();
    }

    fn register_voice(reg: &mut EngineRegistry, name: &str) {
        let worker = worker_script();
        let backend = ProcessBackend::new(
            EngineInfo::new(name, EngineType::Voice),
            vec![worker, name.to_string(), "voice".to_string()],
        );
        reg.register(backend).unwrap();
    }

    #[test]
    fn register_and_activate() {
        let mut reg = EngineRegistry::new();
        register_keyboard(&mut reg, "mock");

        assert_eq!(reg.list_keyboards(), vec!["mock"]);
        reg.activate_keyboard("mock").unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("mock"));
    }

    #[test]
    fn unregister_active_deactivates_first() {
        let mut reg = EngineRegistry::new();
        register_keyboard(&mut reg, "mock");
        reg.activate_keyboard("mock").unwrap();

        reg.unregister("mock").unwrap();
        assert!(reg.active_keyboard_name().is_none());
        assert!(reg.list_keyboards().is_empty());
    }

    #[test]
    fn switch_keyboard_wraps() {
        let mut reg = EngineRegistry::new();
        for name in ["alpha", "beta"] {
            register_keyboard(&mut reg, name);
        }

        reg.activate_keyboard("alpha").unwrap();
        reg.switch_keyboard(SwitchDirection::Next).unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("beta"));
        reg.switch_keyboard(SwitchDirection::Next).unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("alpha"));
    }

    fn register_keyboard_langs(reg: &mut EngineRegistry, name: &str, langs: &[&str]) {
        register_keyboard(reg, name);
        reg.set_engine_languages(name, langs.iter().map(|s| s.to_string()).collect())
            .unwrap();
    }

    fn register_voice_langs(reg: &mut EngineRegistry, name: &str, langs: &[&str]) {
        register_voice(reg, name);
        reg.set_engine_languages(name, langs.iter().map(|s| s.to_string()).collect())
            .unwrap();
    }

    #[test]
    fn known_languages_dedupes_and_skips_pseudo_tags() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans", "zh-Hant"]);
        register_keyboard_langs(&mut reg, "pinyin", &["zh-Hans"]);
        register_keyboard(&mut reg, "untagged"); // defaults to "und"
        register_voice_langs(&mut reg, "whisper", &["mul"]);

        assert_eq!(reg.known_languages(), vec!["zh-Hans", "zh-Hant"]);
    }

    #[test]
    fn resolve_prefers_override_then_declared_order() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
        register_keyboard_langs(&mut reg, "pinyin", &["zh-Hans"]);

        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, None),
            Some("rime".to_string())
        );
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, Some("pinyin")),
            Some("pinyin".to_string())
        );
        // Unregistered override falls back to declared resolution.
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, Some("missing")),
            Some("rime".to_string())
        );
        // "none" forces an empty slot.
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, Some("none")),
            None
        );
    }

    #[test]
    fn resolve_matches_primary_subtag_and_mul() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "anthy", &["ja"]);
        register_voice_langs(&mut reg, "whisper", &["mul"]);

        assert_eq!(
            reg.resolve_language_engine("ja-JP", EngineType::Keyboard, None),
            Some("anthy".to_string())
        );
        assert_eq!(
            reg.resolve_language_engine("ar-MA", EngineType::Voice, None),
            Some("whisper".to_string())
        );
        assert_eq!(
            reg.resolve_language_engine("ar-MA", EngineType::Keyboard, None),
            None
        );
        // No accidental prefix matches without a '-' boundary.
        register_keyboard_langs(&mut reg, "zhuyin", &["zh"]);
        assert_eq!(
            reg.resolve_language_engine("zha", EngineType::Keyboard, None),
            None
        );
    }

    #[test]
    fn activate_language_layout_only_deactivates_keyboard() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);

        reg.activate_language("zh-Hans", None, None).unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("rime"));
        assert_eq!(reg.active_language(), Some("zh-Hans"));

        // Layout-only language (e.g. Moroccan Darija): no engine declares it,
        // so the keyboard slot empties and keys pass through.
        reg.activate_language("ar-MA", None, None).unwrap();
        assert_eq!(reg.active_keyboard_name(), None);
        assert_eq!(reg.active_language(), Some("ar-MA"));
    }

    #[test]
    fn cycle_language_wraps_and_recovers_unknown_active() {
        let enabled: Vec<String> = ["zh-Hans", "en", "ar-MA"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);

        // No active language yet: both directions land on the first entry.
        assert_eq!(
            reg.cycle_language(&enabled, SwitchDirection::Next)
                .as_deref(),
            Some("zh-Hans")
        );

        reg.activate_language("zh-Hans", None, None).unwrap();
        assert_eq!(
            reg.cycle_language(&enabled, SwitchDirection::Next)
                .as_deref(),
            Some("en")
        );
        assert_eq!(
            reg.cycle_language(&enabled, SwitchDirection::Previous)
                .as_deref(),
            Some("ar-MA")
        );
        assert_eq!(reg.cycle_language(&[], SwitchDirection::Next), None);
    }

    #[test]
    fn active_language_persists_across_registries() {
        let dir = std::env::temp_dir().join(format!(
            "typio-libtypio-language-state-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let dir_str = dir.to_string_lossy().into_owned();
        let _ = fs::remove_dir_all(&dir);

        let mut reg = EngineRegistry::new();
        reg.set_state_dir(&dir_str);
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
        reg.activate_language("zh-Hans", None, None).unwrap();

        let mut resumed = EngineRegistry::new();
        resumed.set_state_dir(&dir_str);
        assert_eq!(resumed.last_used_language(), Some("zh-Hans"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn last_used_engine_resume_is_category_scoped() {
        let mut reg = EngineRegistry::new();
        register_voice(&mut reg, "recorder");
        register_keyboard(&mut reg, "rime");

        reg.recent_keyboard.primary = Some("recorder".to_string());
        reg.recent_keyboard.secondary = Some("rime".to_string());
        reg.recent_voice.primary = Some("recorder".to_string());

        reg.activate_last_used_keyboard().unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("rime"));

        reg.activate_last_used_voice().unwrap();
        assert_eq!(reg.active_voice_name(), Some("recorder"));
    }

    #[test]
    fn activate_keyboard_reconciles_active_language() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "basic", &["en"]);
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);

        // Activating the English engine tracks it as the active language.
        reg.activate_keyboard("basic").unwrap();
        assert_eq!(reg.active_language(), Some("en"));

        // Switching *directly* to rime (engine path, not activate_language)
        // must retarget the active language instead of leaving it "en".
        reg.activate_keyboard("rime").unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("rime"));
        assert_eq!(reg.active_language(), Some("zh-Hans"));

        // Switching back flips the active language again.
        reg.activate_keyboard("basic").unwrap();
        assert_eq!(reg.active_language(), Some("en"));
    }

    #[test]
    fn activate_keyboard_same_language_does_not_churn() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
        register_keyboard_langs(&mut reg, "pinyin", &["zh-Hans"]);

        reg.activate_language("zh-Hans", None, None).unwrap();
        assert_eq!(reg.active_language(), Some("zh-Hans"));

        // Switching between two zh-Hans engines keeps the tag stable.
        reg.activate_keyboard("pinyin").unwrap();
        assert_eq!(reg.active_language(), Some("zh-Hans"));
    }

    #[test]
    fn activate_keyboard_matches_primary_subtag() {
        // active_language "zh" and an engine declaring "zh-Hans" are the same
        // language (BCP-47 prefix match) and must not be treated as a change.
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);

        reg.active_language = Some("zh".to_string());
        reg.activate_keyboard("rime").unwrap();
        assert_eq!(reg.active_language(), Some("zh"));
    }

    #[test]
    fn activate_keyboard_und_engine_leaves_active_language() {
        // An untagged engine carries no language info; keep the previous tag
        // rather than retargeting to the meaningless "und".
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
        register_keyboard(&mut reg, "raw"); // defaults to "und"

        reg.activate_keyboard("rime").unwrap();
        assert_eq!(reg.active_language(), Some("zh-Hans"));

        reg.activate_keyboard("raw").unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("raw"));
        assert_eq!(reg.active_language(), Some("zh-Hans"));
    }

    #[test]
    fn switch_keyboard_reconciles_active_language() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "basic", &["en"]);
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);

        reg.activate_keyboard("basic").unwrap();
        reg.switch_keyboard(SwitchDirection::Next).unwrap();
        assert_eq!(reg.active_keyboard_name(), Some("rime"));
        assert_eq!(reg.active_language(), Some("zh-Hans"));
    }

    #[test]
    fn activate_voice_does_not_move_active_language() {
        // The badge reflects the keyboard language; switching a voice engine
        // independently must not retarget the active language.
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "basic", &["en"]);
        register_voice_langs(&mut reg, "whisper-zh", &["zh"]);

        reg.activate_keyboard("basic").unwrap();
        assert_eq!(reg.active_language(), Some("en"));

        reg.activate_voice("whisper-zh").unwrap();
        assert_eq!(reg.active_voice_name(), Some("whisper-zh"));
        assert_eq!(reg.active_language(), Some("en"));
    }

    #[test]
    fn resolve_language_prefers_last_used_engine() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]); // registered first
        register_keyboard_langs(&mut reg, "pinyin", &["zh-Hans"]);

        // Default: registration order picks rime.
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, None),
            Some("rime".to_string())
        );

        // After the user switches to pinyin, zh-Hans must remember pinyin.
        reg.activate_keyboard("pinyin").unwrap();
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, None),
            Some("pinyin".to_string())
        );

        // Switching back to rime remembers rime again.
        reg.activate_keyboard("rime").unwrap();
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, None),
            Some("rime".to_string())
        );
    }

    #[test]
    fn resolve_override_beats_last_used_engine() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
        register_keyboard_langs(&mut reg, "pinyin", &["zh-Hans"]);
        reg.activate_keyboard("pinyin").unwrap(); // pinyin is remembered

        // An explicit override still wins over the remembered engine.
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, Some("rime")),
            Some("rime".to_string())
        );
    }

    #[test]
    fn resolve_last_used_matches_primary_subtag() {
        // A memory keyed on the short tag "zh" must satisfy a "zh-Hans"
        // request (BCP-47 prefix match), so the short tag is reused.
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh"]);
        register_keyboard_langs(&mut reg, "pinyin", &["zh"]);
        reg.activate_keyboard("rime").unwrap();
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, None),
            Some("rime".to_string())
        );
    }

    #[test]
    fn resolve_last_used_falls_back_when_engine_unregistered() {
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
        register_keyboard_langs(&mut reg, "pinyin", &["zh-Hans"]);
        reg.activate_keyboard("pinyin").unwrap();

        reg.unregister("pinyin").unwrap(); // remembered engine is gone
        assert_eq!(
            reg.resolve_language_engine("zh-Hans", EngineType::Keyboard, None),
            Some("rime".to_string()) // back to registration order
        );
    }

    #[test]
    fn last_used_engine_persists_across_registries() {
        let dir = std::env::temp_dir().join(format!(
            "typio-libtypio-lang-engine-state-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let dir_str = dir.to_string_lossy().into_owned();
        let _ = fs::remove_dir_all(&dir);

        {
            let mut reg = EngineRegistry::new();
            reg.set_state_dir(&dir_str);
            register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
            register_keyboard_langs(&mut reg, "pinyin", &["zh-Hans"]);
            reg.activate_keyboard("pinyin").unwrap();
        }

        // A fresh registry reloads the persisted per-language memory.
        let mut resumed = EngineRegistry::new();
        resumed.set_state_dir(&dir_str);
        register_keyboard_langs(&mut resumed, "rime", &["zh-Hans"]);
        register_keyboard_langs(&mut resumed, "pinyin", &["zh-Hans"]);
        assert_eq!(
            resumed.resolve_language_engine("zh-Hans", EngineType::Keyboard, None),
            Some("pinyin".to_string())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cycle_language_single_language_returns_none() {
        // With fewer than two languages there is nothing to cycle; the host
        // uses this to fall back to engine cycling.
        let one: Vec<String> = vec!["zh-Hans".to_string()];
        let mut reg = EngineRegistry::new();
        register_keyboard_langs(&mut reg, "rime", &["zh-Hans"]);
        reg.activate_language("zh-Hans", None, None).unwrap();
        assert_eq!(reg.cycle_language(&one, SwitchDirection::Next), None);
        assert_eq!(reg.cycle_language(&[], SwitchDirection::Next), None);
    }
}
