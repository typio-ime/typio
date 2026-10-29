//! Key binding parsing and matching.
//!
//! A binding is declared once, as text, in `core.toml`
//! (`[shortcuts] voice_ptt = "Super+v"`). The host parses it into a
//! [`KeyBinding`] and matches key events against that value. Nothing on the
//! keystroke path compares against hardcoded keysyms, because a documented
//! setting that the frontend never consults is worse than no setting: it turns a
//! missing capability into a silent misconfiguration (ADR-0056).
//!
//! # Grammar
//!
//! ```text
//! binding  := modifier ( "+" modifier )* ( "+" key )?
//! modifier := "ctrl" | "control" | "alt" | "shift" | "super" | "meta"
//! key      := one printable character, a case-insensitive letter, an
//!             xkb keysym name from the table below, or a single-letter name
//! ```
//!
//! Modifier names are case-insensitive; spaces around `+` are ignored. A binding
//! with no key is a *modifier chord* (the `Ctrl+Shift` engine-switch gesture).
//! The empty string (or `none`/`disabled`) is [`KeyBinding::NONE`], which never
//! matches.
//!
//! # Why a table instead of `xkb_keysym_from_name`
//!
//! Parsing happens once per config generation, off the keystroke path, and the
//! set of bindable names is deliberately small: a shortcut that names a keysym
//! we cannot express is better rejected at parse time with a clear message than
//! accepted and then never matched. The table is the contract.
//!
//! # Matching is layout-exact for named keys and layout-tolerant for letters
//!
//! A letter binding matches both the lowercase and uppercase keysym, because a
//! shift-free `v` and a `Shift+V` both produce the "v" key and the modifier mask
//! is compared separately. Named keys match only their documented keysym.

use crate::modifiers::Modifiers;

/// Numeric keysym, matching `xkb_keysym_t`.
pub type Keysym = u32;

/// A parsed keyboard binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    /// Required modifier mask. Matching uses equality against this mask, so a
    /// chord with extra modifiers held does not match.
    pub modifiers: Modifiers,
    /// Trigger keysym, or `0` for a modifier-only chord.
    pub keysym: Keysym,
}

impl KeyBinding {
    /// A binding that never matches.
    pub const NONE: Self = Self {
        modifiers: Modifiers::NONE,
        keysym: 0,
    };

    /// True when this binding can never match anything.
    pub fn is_unbound(self) -> bool {
        self.keysym == 0 && self.modifiers == Modifiers::NONE
    }

    /// True when this is a modifier-only chord (no trigger key).
    pub fn is_modifier_chord(self) -> bool {
        self.keysym == 0 && !self.is_unbound()
    }

    /// Does a key event match this binding?
    ///
    /// `held` is the effective modifier mask at event time. The comparison is
    /// exact: holding an extra modifier makes the chord not match, so
    /// `Ctrl+Super+v` does not fire a `Super+v` binding.
    ///
    /// A modifier-only chord never matches a key event; use
    /// [`Self::matches_modifier_release`] for the gesture form.
    pub fn matches_key(self, keysym: Keysym, held: Modifiers) -> bool {
        if self.is_unbound() || self.is_modifier_chord() {
            return false;
        }
        if held != self.modifiers {
            return false;
        }
        if keysym == self.keysym {
            return true;
        }
        // Letter bindings accept either case: `v` and `V` are the same key and
        // the shift state is already accounted for in `modifiers`.
        matches!(
            (lowercase_letter(self.keysym), lowercase_letter(keysym)),
            (Some(want), Some(got)) if want == got
        )
    }

    /// True when `released` is the last modifier of this chord coming up.
    ///
    /// Used by the release-triggered gesture form: a modifier chord fires when
    /// one of its modifiers is released while every other required modifier is
    /// still held. That ordering is what keeps `Ctrl+Shift+V` from firing the
    /// engine switch on the press that merely completes the modifier set.
    pub fn matches_modifier_release(self, released: Modifiers, still_held: Modifiers) -> bool {
        if !self.is_modifier_chord() {
            return false;
        }
        self.modifiers.intersects(released) && still_held.intersects(self.modifiers)
    }
}

/// Parse a binding from its configured text.
///
/// # Errors
/// Returns a message naming the offending token. Callers surface this at
/// startup or config-reload time; silently falling back to a default would
/// reproduce the "I rebound it and nothing happened" failure this module exists
/// to remove.
pub fn parse(text: &str) -> Result<KeyBinding, String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("none")
        || trimmed.eq_ignore_ascii_case("disabled")
    {
        return Ok(KeyBinding::NONE);
    }

    let mut modifiers = Modifiers::NONE;
    let mut keysym: Option<Keysym> = None;

    for token in trimmed.split('+') {
        let token = token.trim();
        if token.is_empty() {
            return Err(format!("empty component in binding {text:?}"));
        }
        if let Some(bit) = modifier_named(token) {
            if modifiers.intersects(bit) {
                return Err(format!("duplicate modifier {token:?} in binding {text:?}"));
            }
            modifiers = Modifiers(modifiers.0 | bit.0);
            continue;
        }
        if keysym.is_some() {
            return Err(format!(
                "binding {text:?} names more than one key; the last is {token:?}"
            ));
        }
        keysym = Some(keysym_named(token).ok_or_else(|| {
            format!(
                "unknown key {token:?} in binding {text:?}; see docs/reference/configuration.md"
            )
        })?);
    }

    Ok(KeyBinding {
        modifiers,
        keysym: keysym.unwrap_or(0),
    })
}

/// Map a modifier token to its bit.
fn modifier_named(token: &str) -> Option<Modifiers> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(Modifiers::CTRL),
        "alt" | "option" => Some(Modifiers::ALT),
        "shift" => Some(Modifiers::SHIFT),
        "super" | "meta" | "win" | "cmd" => Some(Modifiers::SUPER),
        _ => None,
    }
}

/// Keysyms with a stable name. Deliberately small and explicit: see the module
/// docs for why this is a table rather than a full xkb lookup.
///
/// Every entry is the canonical lowercase `xkbkeysym` spelling. Lookup
/// lowercases the token, so `Escape` and `escape` both resolve.
const NAMED_KEYS: &[(&str, Keysym)] = &[
    ("space", 0x0020),
    ("tab", 0xff09),
    ("escape", 0xff1b),
    ("esc", 0xff1b),
    ("return", 0xff0d),
    ("enter", 0xff0d),
    ("backspace", 0xff08),
    ("delete", 0xffff),
    ("insert", 0xff63),
    ("home", 0xff50),
    ("end", 0xff57),
    ("page_up", 0xff55),
    ("pageup", 0xff55),
    ("page_down", 0xff56),
    ("pagedown", 0xff56),
    ("left", 0xff51),
    ("up", 0xff52),
    ("right", 0xff53),
    ("down", 0xff54),
    ("f1", 0xffbe),
    ("f2", 0xffbf),
    ("f3", 0xffc0),
    ("f4", 0xffc1),
    ("f5", 0xffc2),
    ("f6", 0xffc3),
    ("f7", 0xffc4),
    ("f8", 0xffc5),
    ("f9", 0xffc6),
    ("f10", 0xffc7),
    ("f11", 0xffc8),
    ("f12", 0xffc9),
];

/// Resolve a key token to a keysym.
fn keysym_named(token: &str) -> Option<Keysym> {
    // A single character binds that character's Latin-1 keysym.
    let mut chars = token.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if let Some(keysym) = latin1_keysym(ch) {
            return Some(keysym);
        }
    }
    let lowered = token.to_ascii_lowercase();
    NAMED_KEYS
        .iter()
        .find(|(name, _)| *name == lowered)
        .map(|(_, keysym)| *keysym)
}

/// Latin-1 keysym for a printable ASCII character.
fn latin1_keysym(ch: char) -> Option<Keysym> {
    let code = ch as u32;
    // Printable ASCII maps to itself in the Latin-1 keysym range; control
    // characters are not bindable so that a stray newline is a parse error
    // rather than a magic binding.
    (0x20..=0x7e).contains(&code).then_some(code)
}

/// The lowercase form of a letter keysym, for case-insensitive letter matching.
fn lowercase_letter(keysym: Keysym) -> Option<char> {
    let ch = char::from_u32(keysym)?;
    ch.is_ascii_alphabetic().then(|| ch.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> KeyBinding {
        parse(text).unwrap_or_else(|error| panic!("{text:?} should parse: {error}"))
    }

    #[test]
    fn parses_the_documented_defaults() {
        assert_eq!(
            p("Super+v"),
            KeyBinding {
                modifiers: Modifiers::SUPER,
                keysym: 0x0076
            }
        );
        assert_eq!(
            p("Ctrl+Shift"),
            KeyBinding {
                modifiers: Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0),
                keysym: 0
            }
        );
        assert_eq!(
            p("Ctrl+Shift+Escape"),
            KeyBinding {
                modifiers: Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0),
                keysym: 0xff1b
            }
        );
        assert_eq!(
            p("Ctrl+Super+i"),
            KeyBinding {
                modifiers: Modifiers(Modifiers::CTRL.0 | Modifiers::SUPER.0),
                keysym: 0x0069
            }
        );
    }

    #[test]
    fn modifier_names_are_case_insensitive_and_spaced() {
        assert_eq!(p("SUPER+v"), p("super+v"));
        assert_eq!(p("ctrl + shift + escape"), p("Ctrl+Shift+Escape"));
        assert_eq!(p("Control+Alt+Delete"), p("ctrl+alt+delete"));
        assert_eq!(p("Meta+v"), p("Super+v"));
        assert_eq!(p("Win+v"), p("Super+v"));
    }

    #[test]
    fn empty_and_disabled_bindings_never_match() {
        for text in ["", "  ", "none", "NONE", "disabled"] {
            let binding = p(text);
            assert!(binding.is_unbound(), "{text:?} should be unbound");
            assert!(!binding.matches_key(0x0076, Modifiers::SUPER));
        }
    }

    #[test]
    fn unknown_key_is_a_named_error() {
        let error = parse("Super+nosuchkey").expect_err("must reject");
        assert!(error.contains("unknown key"), "{error}");
        assert!(error.contains("nosuchkey"), "{error}");
        assert!(error.contains("docs/reference/configuration.md"), "{error}");
    }

    #[test]
    fn duplicate_modifier_is_rejected() {
        let error = parse("Ctrl+Ctrl+v").expect_err("must reject");
        assert!(error.contains("duplicate modifier"), "{error}");
    }

    #[test]
    fn two_keys_is_rejected() {
        let error = parse("Ctrl+v+w").expect_err("must reject");
        assert!(error.contains("more than one key"), "{error}");
    }

    #[test]
    fn empty_component_is_rejected() {
        assert!(parse("Ctrl++v").is_err());
        assert!(parse("+v").is_err());
    }

    #[test]
    fn modifier_chord_matches_exactly() {
        let chord = p("Ctrl+Shift");
        assert!(chord.is_modifier_chord());
        // A modifier chord never matches a key event, regardless of modifiers.
        assert!(!chord.matches_key(0x0076, Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0)));
        assert!(!chord.matches_key(0xffe1, Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0)));
    }

    #[test]
    fn key_binding_matches_exactly_and_rejects_extra_modifiers() {
        let binding = p("Super+v");
        assert!(binding.matches_key(0x0076, Modifiers::SUPER));
        // Extra modifier held: must not fire, or Ctrl+Super+V would trigger it.
        assert!(!binding.matches_key(0x0076, Modifiers(Modifiers::SUPER.0 | Modifiers::CTRL.0)));
        // Wrong key.
        assert!(!binding.matches_key(0x0062, Modifiers::SUPER));
        // No modifiers.
        assert!(!binding.matches_key(0x0076, Modifiers::NONE));
    }

    #[test]
    fn letter_bindings_accept_either_case() {
        let binding = p("Super+v");
        assert!(binding.matches_key(0x0076, Modifiers::SUPER), "lowercase v");
        assert!(binding.matches_key(0x0056, Modifiers::SUPER), "uppercase V");

        let upper = p("Super+V");
        assert!(upper.matches_key(0x0076, Modifiers::SUPER));
        assert!(upper.matches_key(0x0056, Modifiers::SUPER));
    }

    #[test]
    fn named_keys_match_only_their_keysym() {
        let binding = p("Ctrl+Shift+Escape");
        assert!(binding.matches_key(0xff1b, Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0)));
        assert!(!binding.matches_key(0x0076, Modifiers(Modifiers::CTRL.0 | Modifiers::SHIFT.0)));
    }

    #[test]
    fn modifier_release_chord_fires_only_while_the_rest_is_held() {
        let chord = p("Ctrl+Shift");
        // Releasing Ctrl while Shift is still held: fires.
        assert!(chord.matches_modifier_release(Modifiers::CTRL, Modifiers::SHIFT));
        // Releasing Ctrl with nothing else held: does not fire.
        assert!(!chord.matches_modifier_release(Modifiers::CTRL, Modifiers::NONE));
        // Releasing a modifier that is not part of the chord: does not fire.
        assert!(!chord.matches_modifier_release(Modifiers::ALT, Modifiers::CTRL));
        // A key binding is not a modifier chord.
        assert!(!p("Super+v").matches_modifier_release(Modifiers::SUPER, Modifiers::NONE));
    }

    #[test]
    fn a_rebound_chord_actually_changes_the_match() {
        // The whole point: a user's binding decides, not a hardcoded constant.
        let custom = p("Ctrl+Alt+space");
        assert!(custom.matches_key(0x0020, Modifiers(Modifiers::CTRL.0 | Modifiers::ALT.0)));
        assert!(!custom.matches_key(0x0076, Modifiers::SUPER));
    }
}
