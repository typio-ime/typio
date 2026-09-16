use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use iris::{Align, Frame, LayoutOpts, TextBuf};
use serde_json::json;

use crate::events::{Event, EventWorker};
use crate::model::{ConfigEntry, Snapshot, rpc};
use crate::platform_config::{DisplaySettings, FONT_FAMILIES, PlatformConfig};

/// Menu labels for [`FONT_FAMILIES`], in the same order.
const FONT_FAMILY_LABELS: [&str; 4] = ["System default", "Sans", "Serif", "Monospace"];

const REFRESH_RETRY: Duration = Duration::from_secs(2);
const PLATFORM_SAVE_DELAY: Duration = Duration::from_millis(250);

enum Action {
    UseLanguage(String),
    UseEngine { kind: String, name: String },
    SetConfig { key: String, value: String },
    UnsetConfig(String),
    Invoke { engine: String, command: String },
    Refresh,
}

pub struct AppState {
    page: i32,
    snapshot: Snapshot,
    connected: bool,
    status: String,
    last_refresh: Instant,
    events: EventWorker,
    buffers: HashMap<String, TextBuf>,
    dirty: HashSet<String>,
    expanded_sections: HashSet<String>,

    platform: Option<PlatformConfig>,
    platform_error: Option<String>,
    platform_save_at: Option<Instant>,
    theme: i32,
    candidate_layout: i32,
    font_size: f32,
    /// Index into [`FONT_FAMILIES`].
    font_family: i32,
    panel_mode_indicator: bool,
    anchor_probe: bool,
    anchor_probe_timeout_ms: f32,
}

impl AppState {
    pub fn new() -> Self {
        let (platform, platform_error) = match PlatformConfig::load() {
            Ok(config) => (Some(config), None),
            Err(error) => (None, Some(error.to_string())),
        };

        let theme = platform
            .as_ref()
            .map(|config| match config.theme() {
                "light" => 1,
                "dark" => 2,
                _ => 0,
            })
            .unwrap_or(0);
        let candidate_layout = platform
            .as_ref()
            .map(|config| i32::from(config.candidate_layout() == "vertical"))
            .unwrap_or(0);
        let font_size = platform
            .as_ref()
            .map(|config| config.font_size() as f32)
            .unwrap_or(11.0);
        let font_family = FONT_FAMILIES
            .iter()
            .position(|candidate| {
                platform
                    .as_ref()
                    .is_some_and(|config| config.font_family() == *candidate)
            })
            .unwrap_or(0) as i32;
        let panel_mode_indicator = platform
            .as_ref()
            .is_some_and(PlatformConfig::panel_mode_indicator);
        let anchor_probe = platform.as_ref().is_none_or(PlatformConfig::anchor_probe);
        let anchor_probe_timeout_ms = platform
            .as_ref()
            .map(|config| config.anchor_probe_timeout_ms() as f32)
            .unwrap_or(150.0);

        let mut state = Self {
            page: 0,
            snapshot: Snapshot::default(),
            connected: false,
            status: String::new(),
            last_refresh: Instant::now() - REFRESH_RETRY,
            events: EventWorker::start(),
            buffers: HashMap::new(),
            dirty: HashSet::new(),
            expanded_sections: HashSet::new(),
            platform,
            platform_error,
            platform_save_at: None,
            theme,
            candidate_layout,
            font_size,
            font_family,
            panel_mode_indicator,
            anchor_probe,
            anchor_probe_timeout_ms,
        };
        state.refresh();
        state
    }

    pub fn frame(&mut self, frame: &mut Frame) {
        self.poll_events();
        self.flush_platform_if_due();

        let mut action = None;
        let outer = LayoutOpts {
            flex: 1.0,
            gap: 12.0,
            pad: 16.0,
            ..LayoutOpts::default()
        };
        frame.column_ex(&outer, |frame| {
            frame.row_ex(
                &LayoutOpts {
                    gap: 12.0,
                    cross: Align::Center,
                    ..LayoutOpts::default()
                },
                |frame| {
                    frame.heading("Typio Settings", 1);
                    frame.flex(1.0);
                    frame.spacer(1.0);
                    if self.connected {
                        frame.label_sized(
                            &format!("Connected · {}", self.snapshot.daemon_version),
                            12.0,
                        );
                    } else {
                        frame.label_sized("Daemon unavailable", 12.0);
                    }
                    if frame.button("Refresh") {
                        action = Some(Action::Refresh);
                    }
                },
            );
            frame.separator();
            frame.row_ex(
                &LayoutOpts {
                    flex: 1.0,
                    gap: 12.0,
                    ..LayoutOpts::default()
                },
                |frame| {
                    self.sidebar(frame);
                    frame.flex(1.0);
                    frame.scroll("settings-content", |frame| {
                        frame.column_ex(
                            &LayoutOpts {
                                gap: 12.0,
                                pad: 4.0,
                                ..LayoutOpts::default()
                            },
                            |frame| match self.page {
                                0 => self.appearance_page(frame),
                                1 => self.input_page(frame, &mut action),
                                2 => self.shortcuts_page(frame, &mut action),
                                _ => self.advanced_page(frame, &mut action),
                            },
                        );
                    });
                },
            );
            frame.separator();
            frame.label_sized(&self.status, 12.0);
        });

        if let Some(action) = action {
            self.apply(action);
        }
    }

    fn sidebar(&mut self, frame: &mut Frame) {
        frame.column_ex(
            &LayoutOpts {
                width: 190.0,
                gap: 4.0,
                pad: 4.0,
                ..LayoutOpts::default()
            },
            |frame| {
                for (index, label) in ["Appearance", "Input", "Shortcuts", "Advanced"]
                    .iter()
                    .enumerate()
                {
                    if frame.selectable(label, self.page == index as i32) {
                        self.page = index as i32;
                    }
                }
            },
        );
    }

    fn appearance_page(&mut self, frame: &mut Frame) {
        frame.heading("Appearance", 2);
        frame.label("Panel rendering and candidate-window behavior.");
        if let Some(error) = &self.platform_error {
            frame.label_sized(&format!("platform.toml is not editable: {error}"), 12.0);
        }

        let mut changed = false;
        changed |= frame.dropdown("Theme", &mut self.theme, &["System", "Light", "Dark"]);
        changed |= frame.dropdown(
            "Candidate layout",
            &mut self.candidate_layout,
            &["Horizontal", "Vertical"],
        );
        changed |= frame.slider("Font size", &mut self.font_size, 6.0, 72.0);
        frame.label_sized(&format!("{:.0} pt", self.font_size), 12.0);
        changed |= frame.dropdown("Font family", &mut self.font_family, &FONT_FAMILY_LABELS);
        changed |= frame.checkbox(
            "Show input mode in the panel",
            &mut self.panel_mode_indicator,
        );

        frame.separator();
        frame.heading("Placement", 2);
        changed |= frame.checkbox("Probe the compositor anchor", &mut self.anchor_probe);
        changed |= frame.slider(
            "Anchor probe timeout",
            &mut self.anchor_probe_timeout_ms,
            25.0,
            1000.0,
        );
        frame.label_sized(&format!("{:.0} ms", self.anchor_probe_timeout_ms), 12.0);

        if changed {
            self.platform_save_at = Some(Instant::now() + PLATFORM_SAVE_DELAY);
            self.status = "Saving panel settings…".to_owned();
        }
    }

    fn input_page(&mut self, frame: &mut Frame, action: &mut Option<Action>) {
        frame.heading("Input", 2);
        frame.label("Choose the active language and engines, then tune engine options.");

        if !self.snapshot.languages.is_empty() {
            let labels: Vec<String> = self
                .snapshot
                .languages
                .iter()
                .map(|language| language.tag.clone())
                .collect();
            let items: Vec<&str> = labels.iter().map(String::as_str).collect();
            let mut selected = self
                .snapshot
                .languages
                .iter()
                .position(|language| {
                    language.active || language.tag == self.snapshot.active_language
                })
                .unwrap_or_default() as i32;
            if frame.dropdown("Active language", &mut selected, &items)
                && let Some(language) = self.snapshot.languages.get(selected as usize)
            {
                *action = Some(Action::UseLanguage(language.tag.clone()));
            }
        } else {
            frame.label_sized("No enabled languages were reported by the daemon.", 12.0);
        }

        let input_config: Vec<ConfigEntry> = self
            .snapshot
            .config
            .iter()
            .filter(|entry| {
                entry.key.starts_with("keyboard.")
                    || entry.key.starts_with("voice.")
                    || entry.key.starts_with("languages.")
            })
            .cloned()
            .collect();
        if !input_config.is_empty()
            && Self::section_disclosure(
                &mut self.expanded_sections,
                frame,
                "input_behavior",
                "Input behavior",
            )
        {
            self.config_entries(frame, &input_config, action);
        }

        let engines = self.snapshot.engines.clone();
        for engine in engines {
            let heading = format!("{} · {}", engine.display_name, engine.kind);
            frame.push_id(&engine.name);
            if Self::section_disclosure(&mut self.expanded_sections, frame, &engine.name, &heading)
            {
                frame.label_sized(&engine.name, 12.0);
                if engine.active {
                    frame.label("Active");
                } else if frame.button("Activate") {
                    *action = Some(Action::UseEngine {
                        kind: engine.kind.clone(),
                        name: engine.name.clone(),
                    });
                }

                self.config_entries(frame, &engine.properties, action);
                if !engine.commands.is_empty() {
                    frame.separator();
                    frame.label("Actions");
                    for command in &engine.commands {
                        frame.push_id(&command.id);
                        let label = if command.label.is_empty() {
                            &command.id
                        } else {
                            &command.label
                        };
                        if frame.button(label) {
                            *action = Some(Action::Invoke {
                                engine: engine.name.clone(),
                                command: command.id.clone(),
                            });
                        }
                        frame.pop_id();
                    }
                }
            }
            frame.pop_id();
        }
    }

    fn shortcuts_page(&mut self, frame: &mut Frame, action: &mut Option<Action>) {
        frame.heading("Shortcuts", 2);
        frame.label("Global Typio key bindings. Use accelerator strings such as Control+Space.");
        let entries: Vec<ConfigEntry> = self
            .snapshot
            .config
            .iter()
            .filter(|entry| entry.key.starts_with("shortcuts."))
            .cloned()
            .collect();
        if entries.is_empty() {
            frame.label_sized("No shortcut fields were reported by the daemon.", 12.0);
        } else {
            self.config_entries(frame, &entries, action);
        }
    }

    fn advanced_page(&mut self, frame: &mut Frame, action: &mut Option<Action>) {
        frame.heading("Advanced", 2);
        frame.label("Schema-backed settings not shown on the other pages.");
        let entries: Vec<ConfigEntry> = self
            .snapshot
            .config
            .iter()
            .filter(|entry| {
                !entry.key.starts_with("shortcuts.")
                    && !entry.key.starts_with("engines.")
                    && !entry.key.starts_with("keyboard.")
                    && !entry.key.starts_with("voice.")
                    && !entry.key.starts_with("languages.")
            })
            .cloned()
            .collect();
        if entries.is_empty() {
            frame.label_sized("No additional settings were reported by the daemon.", 12.0);
        } else {
            self.config_entries(frame, &entries, action);
        }

        if !self.snapshot.daemon_status.is_null() {
            frame.separator();
            if Self::section_disclosure(
                &mut self.expanded_sections,
                frame,
                "daemon_status",
                "Daemon status",
            ) {
                frame.label_sized(&self.snapshot.daemon_status.to_string(), 12.0);
            }
        }
    }

    fn section_disclosure(
        expanded: &mut HashSet<String>,
        frame: &mut Frame,
        id: &str,
        label: &str,
    ) -> bool {
        let open = expanded.contains(id);
        let marker = if open { "▾ " } else { "▸ " };
        let button_text = format!("{marker}{label}");
        if frame.button_subtle(&button_text) {
            if open {
                expanded.remove(id);
            } else {
                expanded.insert(id.to_string());
            }
        }
        expanded.contains(id)
    }

    fn config_entries(
        &mut self,
        frame: &mut Frame,
        entries: &[ConfigEntry],
        action: &mut Option<Action>,
    ) {
        for entry in entries {
            frame.push_id(&entry.key);
            frame.label(entry.display_label());
            let section = if entry.section.is_empty() {
                "general"
            } else {
                &entry.section
            };
            frame.label_sized(
                &format!("{} · {} · {section}", entry.key, entry.source),
                11.0,
            );

            if entry.field_type == "bool" {
                let mut value = entry.value.as_bool().unwrap_or(false);
                frame.row_ex(
                    &LayoutOpts {
                        gap: 8.0,
                        cross: Align::Center,
                        ..LayoutOpts::default()
                    },
                    |frame| {
                        if frame.checkbox("Enabled", &mut value) {
                            *action = Some(Action::SetConfig {
                                key: entry.key.clone(),
                                value: value.to_string(),
                            });
                        }
                        if frame.button("Use default") {
                            *action = Some(Action::UnsetConfig(entry.key.clone()));
                        }
                    },
                );
            } else if !entry.choices.is_empty() {
                let mut selected = entry
                    .choices
                    .iter()
                    .position(|choice| choice == &entry.text_value())
                    .unwrap_or_default() as i32;
                let choices: Vec<&str> = entry.choices.iter().map(String::as_str).collect();
                if frame.dropdown("Value", &mut selected, &choices)
                    && let Some(value) = entry.choices.get(selected as usize)
                {
                    *action = Some(Action::SetConfig {
                        key: entry.key.clone(),
                        value: value.clone(),
                    });
                }
                if frame.button("Use default") {
                    *action = Some(Action::UnsetConfig(entry.key.clone()));
                }
            } else {
                let edited = {
                    let buffer = self
                        .buffers
                        .entry(entry.key.clone())
                        .or_insert_with(|| TextBuf::new(1024, &entry.text_value()));
                    if frame.textfield("Value", buffer) {
                        self.dirty.insert(entry.key.clone());
                    }
                    buffer.as_str().into_owned()
                };
                frame.row_ex(
                    &LayoutOpts {
                        gap: 8.0,
                        cross: Align::Center,
                        ..LayoutOpts::default()
                    },
                    |frame| {
                        if frame.button("Apply") {
                            *action = Some(Action::SetConfig {
                                key: entry.key.clone(),
                                value: edited,
                            });
                        }
                        if frame.button("Use default") {
                            *action = Some(Action::UnsetConfig(entry.key.clone()));
                        }
                    },
                );
            }
            frame.separator();
            frame.pop_id();
        }
    }

    fn apply(&mut self, action: Action) {
        if matches!(action, Action::Refresh) {
            self.refresh();
            return;
        }

        let (result, success, edited_key) = match action {
            Action::UseLanguage(tag) => (
                rpc("language.use", json!({ "tag": tag })),
                format!("Active language changed to {tag}"),
                None,
            ),
            Action::UseEngine { kind, name } => {
                let method = if kind == "voice" {
                    "voice.use"
                } else {
                    "keyboard.use"
                };
                (
                    rpc(method, json!({ "name": name })),
                    format!("Active {kind} engine changed to {name}"),
                    None,
                )
            }
            Action::SetConfig { key, value } => (
                rpc("config.set", json!({ "key": key, "value": value })),
                format!("Saved {key}"),
                Some(key),
            ),
            Action::UnsetConfig(key) => (
                rpc("config.unset", json!({ "key": key })),
                format!("Restored the default for {key}"),
                Some(key),
            ),
            Action::Invoke { engine, command } => (
                rpc(
                    "engine.invoke",
                    json!({ "name": engine, "command": command }),
                ),
                format!("Ran {command} for {engine}"),
                None,
            ),
            Action::Refresh => unreachable!(),
        };

        match result {
            Ok(_) => {
                if let Some(key) = edited_key {
                    self.dirty.remove(&key);
                }
                self.refresh();
                self.status = success;
            }
            Err(error) => {
                self.connected = false;
                self.status = format!("Could not apply setting: {error}");
            }
        }
    }

    fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        match Snapshot::load() {
            Ok(snapshot) => {
                for entry in snapshot.config.iter().chain(
                    snapshot
                        .engines
                        .iter()
                        .flat_map(|engine| &engine.properties),
                ) {
                    if !self.dirty.contains(&entry.key) {
                        self.buffers
                            .entry(entry.key.clone())
                            .or_insert_with(|| TextBuf::new(1024, ""))
                            .set(&entry.text_value());
                    }
                }
                self.snapshot = snapshot;
                self.connected = true;
                self.status = "Settings are up to date.".to_owned();
            }
            Err(error) => {
                self.connected = false;
                self.status = error.to_string();
            }
        }
    }

    fn poll_events(&mut self) {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Changed => changed = true,
                Event::Disconnected(error) => {
                    self.connected = false;
                    self.status = format!("Daemon connection lost: {error}");
                }
            }
        }
        if changed || (!self.connected && self.last_refresh.elapsed() >= REFRESH_RETRY) {
            self.refresh();
        }
    }

    fn flush_platform_if_due(&mut self) {
        if self
            .platform_save_at
            .is_none_or(|deadline| Instant::now() < deadline)
        {
            return;
        }
        self.platform_save_at = None;
        let Some(config) = self.platform.as_mut() else {
            self.status = "Cannot save platform settings until platform.toml is fixed.".to_owned();
            return;
        };

        let theme = match self.theme {
            1 => "light",
            2 => "dark",
            _ => "auto",
        };
        let layout = if self.candidate_layout == 1 {
            "vertical"
        } else {
            "horizontal"
        };
        config.update(&DisplaySettings {
            theme,
            layout,
            font_size: self.font_size as f64,
            font_family: FONT_FAMILIES
                [(self.font_family.max(0) as usize).min(FONT_FAMILIES.len() - 1)],
            panel_mode_indicator: self.panel_mode_indicator,
            anchor_probe: self.anchor_probe,
            anchor_probe_timeout_ms: self.anchor_probe_timeout_ms.round() as i64,
        });
        match config.save() {
            Ok(()) => self.status = "Saved panel settings.".to_owned(),
            Err(error) => self.status = format!("Could not save panel settings: {error}"),
        }
    }
}
