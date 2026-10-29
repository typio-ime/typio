//! The host's resolved shortcut bindings.
//!
//! Every shortcut the frontend honours is declared here, resolved once from
//! `core.toml`, and matched through [`KeyBinding`]. Nothing on the keystroke path
//! compares against a hardcoded keysym, and there is no second place a chord can
//! be defined — the previous design had the defaults in the config schema, the
//! matching in the router, and an unused keysym constant in between, which is how
//! four of five documented shortcuts came to do nothing (ADR-0056).
//!
//! # Resolution policy
//!
//! A key missing from the configuration takes its documented default. A key whose
//! value cannot be parsed is reported, and the affected shortcut is left
//! **unbound** rather than reverted to the default: silently running the default
//! is exactly the "I rebound it and nothing happened" failure this module
//! removes. Errors are collected so one bad key does not hide the rest.

use crate::key_binding::{self, KeyBinding};
use crate::modifiers::Modifiers;

/// Config key for cycling the enabled language list.
pub const KEY_SWITCH_LANGUAGE: &str = "shortcuts.switch_language";
/// Config key for cycling the active keyboard engine.
pub const KEY_SWITCH_KEYBOARD_ENGINE: &str = "shortcuts.switch_keyboard_engine";
/// Config key for the emergency exit.
pub const KEY_EXIT: &str = "shortcuts.exit";
/// Config key for voice push-to-talk.
pub const KEY_VOICE_PTT: &str = "shortcuts.voice_ptt";
/// Config key for re-showing the on-screen indicator.
pub const KEY_SUMMON_INDICATOR: &str = "shortcuts.summon_indicator";

/// Every shortcut the frontend honours.
///
/// Adding a variant means adding it to [`Shortcuts::from_lookup`] too, so a new
/// chord cannot exist without a config key behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    /// Cycle the enabled language list (falls back to engine cycling).
    SwitchLanguage,
    /// Cycle to the next keyboard engine.
    SwitchKeyboardEngine,
    /// Emergency daemon shutdown.
    Exit,
    /// Voice push-to-talk.
    VoicePtt,
    /// Re-show the on-screen indicator.
    SummonIndicator,
}

impl Shortcut {
    /// Every shortcut, in declaration order.
    pub const ALL: &'static [Shortcut] = &[
        Shortcut::SwitchLanguage,
        Shortcut::SwitchKeyboardEngine,
        Shortcut::Exit,
        Shortcut::VoicePtt,
        Shortcut::SummonIndicator,
    ];

    /// The `core.toml` key that configures this shortcut.
    pub fn config_key(self) -> &'static str {
        match self {
            Self::SwitchLanguage => KEY_SWITCH_LANGUAGE,
            Self::SwitchKeyboardEngine => KEY_SWITCH_KEYBOARD_ENGINE,
            Self::Exit => KEY_EXIT,
            Self::VoicePtt => KEY_VOICE_PTT,
            Self::SummonIndicator => KEY_SUMMON_INDICATOR,
        }
    }

    /// The documented default binding text.
    ///
    /// Kept here rather than in the daemon so the schema, the documentation, and
    /// the matcher read one source.
    pub fn default_text(self) -> &'static str {
        match self {
            Self::SwitchLanguage => "Ctrl+Shift",
            Self::SwitchKeyboardEngine => "",
            Self::Exit => "Ctrl+Shift+Escape",
            Self::VoicePtt => "Super+v",
            Self::SummonIndicator => "Ctrl+Super+i",
        }
    }

    /// The documented default, parsed.
    ///
    /// # Panics
    /// Panics only if a default in this file is malformed, which the unit tests
    /// assert against.
    pub fn default_binding(self) -> KeyBinding {
        key_binding::parse(self.default_text())
            .unwrap_or_else(|error| panic!("built-in default {:?}: {error}", self.default_text()))
    }

    /// Look up a shortcut by its config key.
    pub fn from_config_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.config_key() == key)
    }
}

/// A problem found while resolving shortcuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutError {
    /// The shortcut whose value was rejected.
    pub shortcut: Shortcut,
    /// The offending text.
    pub value: String,
    /// What was wrong with it.
    pub reason: String,
}

impl std::fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} = {:?}: {}",
            self.shortcut.config_key(),
            self.value,
            self.reason
        )
    }
}

/// Resolved bindings for every shortcut.
///
/// Construct once per config generation with [`Shortcuts::from_lookup`] and share
/// it with the router. It is a plain value, so a config reload builds a new one
/// and swaps it whole — there is no partial-update path that could leave one
/// chord on the old value and another on the new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcuts {
    switch_language: KeyBinding,
    switch_keyboard_engine: KeyBinding,
    exit: KeyBinding,
    voice_ptt: KeyBinding,
    summon_indicator: KeyBinding,
}

impl Default for Shortcuts {
    fn default() -> Self {
        // No lookup entries: an absent key takes its default by construction.
        Self::from_lookup(|_| None).0
    }
}

impl Shortcuts {
    /// The binding for one shortcut.
    pub fn get(&self, shortcut: Shortcut) -> KeyBinding {
        match shortcut {
            Shortcut::SwitchLanguage => self.switch_language,
            Shortcut::SwitchKeyboardEngine => self.switch_keyboard_engine,
            Shortcut::Exit => self.exit,
            Shortcut::VoicePtt => self.voice_ptt,
            Shortcut::SummonIndicator => self.summon_indicator,
        }
    }

    /// Resolve every shortcut from a lookup.
    ///
    /// `lookup` returns the raw configured text for a key, or `None` when the key
    /// is absent. Absent keys take their default; present but malformed keys are
    /// reported in the returned error list **and** leave the shortcut unbound, so
    /// the user sees that their value did not take effect instead of silently
    /// getting the default.
    pub fn from_lookup<F>(lookup: F) -> (Self, Vec<ShortcutError>)
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut errors = Vec::new();
        let mut resolve = |shortcut: Shortcut| -> KeyBinding {
            let Some(text) = lookup(shortcut.config_key()) else {
                return shortcut.default_binding();
            };
            match key_binding::parse(&text) {
                Ok(binding) => binding,
                Err(reason) => {
                    errors.push(ShortcutError {
                        shortcut,
                        value: text,
                        reason,
                    });
                    KeyBinding::NONE
                }
            }
        };
        let resolved = Self {
            switch_language: resolve(Shortcut::SwitchLanguage),
            switch_keyboard_engine: resolve(Shortcut::SwitchKeyboardEngine),
            exit: resolve(Shortcut::Exit),
            voice_ptt: resolve(Shortcut::VoicePtt),
            summon_indicator: resolve(Shortcut::SummonIndicator),
        };
        (resolved, errors)
    }

    /// The first shortcut (in [`Shortcut::ALL`] order) whose key binding matches.
    ///
    /// Moderator-only chords are excluded: they are matched through
    /// [`Self::matching_modifier_release`] on the release that breaks them.
    pub fn matching_key(&self, keysym: u32, held: Modifiers) -> Option<Shortcut> {
        Shortcut::ALL
            .iter()
            .copied()
            .find(|shortcut| self.get(*shortcut).matches_key(keysym, held))
    }

    /// The modifier chord that has just been broken by a release.
    pub fn matching_modifier_release(
        &self,
        released: Modifiers,
        still_held: Modifiers,
    ) -> Option<Shortcut> {
        Shortcut::ALL.iter().copied().find(|shortcut| {
            self.get(*shortcut)
                .matches_modifier_release(released, still_held)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_defaults_all_parse() {
        for shortcut in Shortcut::ALL {
            let binding = shortcut.default_binding();
            if shortcut.default_text().is_empty() {
                assert!(
                    binding.is_unbound(),
                    "{shortcut:?} default should be unbound"
                );
            } else {
                assert!(
                    !binding.is_unbound(),
                    "{shortcut:?} default {:?} resolved to NONE",
                    shortcut.default_text()
                );
            }
        }
    }

    #[test]
    fn documented_defaults_match_the_reference_page() {
        // These literals are duplicated intentionally: if someone changes a
        // default here without updating the reference page (or vice versa), this
        // test names the drift.
        assert_eq!(Shortcut::SwitchLanguage.default_text(), "Ctrl+Shift");
        assert_eq!(Shortcut::SwitchKeyboardEngine.default_text(), "");
        assert_eq!(Shortcut::Exit.default_text(), "Ctrl+Shift+Escape");
        assert_eq!(Shortcut::VoicePtt.default_text(), "Super+v");
        assert_eq!(Shortcut::SummonIndicator.default_text(), "Ctrl+Super+i");
    }

    #[test]
    fn absent_keys_take_defaults() {
        let shortcuts = Shortcuts::default();
        assert!(
            shortcuts
                .get(Shortcut::VoicePtt)
                .matches_key(0x0076, Modifiers::SUPER)
        );
        assert!(
            shortcuts.get(Shortcut::SwitchKeyboardEngine).is_unbound(),
            "documented default is unbound"
        );
    }

    #[test]
    fn a_configured_value_overrides_the_default() {
        let (shortcuts, errors) = Shortcuts::from_lookup(|key| {
            (key == KEY_VOICE_PTT).then(|| "Ctrl+Alt+space".to_string())
        });
        assert!(errors.is_empty(), "{errors:?}");
        assert!(
            shortcuts
                .get(Shortcut::VoicePtt)
                .matches_key(0x0020, Modifiers(Modifiers::CTRL.0 | Modifiers::ALT.0))
        );
        assert!(
            !shortcuts
                .get(Shortcut::VoicePtt)
                .matches_key(0x0076, Modifiers::SUPER),
            "the default Super+v must stop working once rebound"
        );
    }

    #[test]
    fn a_malformed_value_is_reported_and_left_unbound() {
        let (shortcuts, errors) =
            Shortcuts::from_lookup(|key| (key == KEY_EXIT).then(|| "Ctrl+NotAKey".to_string()));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].shortcut, Shortcut::Exit);
        assert!(errors[0].to_string().contains("shortcuts.exit"));
        assert!(
            shortcuts.get(Shortcut::Exit).is_unbound(),
            "a rejected value must not silently fall back to the default"
        );
    }

    #[test]
    fn every_bad_key_is_reported_not_just_the_first() {
        let (_, errors) = Shortcuts::from_lookup(|_| Some("Ctrl+Bogus".to_string()));
        assert_eq!(errors.len(), Shortcut::ALL.len(), "all keys should report");
    }

    #[test]
    fn matching_key_finds_the_configured_shortcut() {
        let shortcuts = Shortcuts::default();
        assert_eq!(
            shortcuts.matching_key(0x0076, Modifiers::SUPER),
            Some(Shortcut::VoicePtt)
        );
        assert_eq!(
            shortcuts.matching_key(0xff1b, Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0)),
            Some(Shortcut::Exit)
        );
        assert_eq!(shortcuts.matching_key(0x0062, Modifiers::NONE), None);
    }

    #[test]
    fn modifier_chords_are_matched_on_release_not_press() {
        let shortcuts = Shortcuts::default();
        // Ctrl+Shift is a modifier chord: it must not match a key event.
        assert_eq!(
            shortcuts.matching_key(0x0076, Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0)),
            None
        );
        assert_eq!(
            shortcuts.matching_modifier_release(Modifiers::CTRL, Modifiers::SHIFT),
            Some(Shortcut::SwitchLanguage)
        );
    }

    #[test]
    fn a_rebound_modifier_chord_changes_the_gesture() {
        let (shortcuts, errors) = Shortcuts::from_lookup(|key| {
            (key == KEY_SWITCH_LANGUAGE).then(|| "Alt+Shift".to_string())
        });
        assert!(errors.is_empty());
        assert_eq!(
            shortcuts.matching_modifier_release(Modifiers::ALT, Modifiers::SHIFT),
            Some(Shortcut::SwitchLanguage)
        );
        assert_eq!(
            shortcuts.matching_modifier_release(Modifiers::CTRL, Modifiers::SHIFT),
            None,
            "the old Ctrl chord must stop firing"
        );
    }

    #[test]
    fn config_key_round_trips() {
        for shortcut in Shortcut::ALL {
            assert_eq!(
                Shortcut::from_config_key(shortcut.config_key()),
                Some(*shortcut)
            );
        }
        assert_eq!(Shortcut::from_config_key("shortcuts.nope"), None);
    }
}
