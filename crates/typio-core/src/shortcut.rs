//! Configurable keyboard shortcut bindings — parsing, defaults, and lookup.
//!
//! Parses shortcut strings (e.g. "Ctrl+Shift", "Super+v") into modifier
//! bitmasks and optional keysyms. Bindings are addressed by stable action
//! IDs (e.g. "switch_language", "voice_ptt") rather than hard-coded struct
//! fields, so adding a new action is an additive change.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use crate::config::{Config, typio_config_get_string};
use crate::types::TypioModifier;

/* ── XKB keysym constants (copied so core needs no xkbcommon dependency) ── */

const XKB_KEY_SPACE: u32 = 0x0020;
const XKB_KEY_BACK_SPACE: u32 = 0xff08;
const XKB_KEY_TAB: u32 = 0xff09;
const XKB_KEY_RETURN: u32 = 0xff0d;
const XKB_KEY_ESCAPE: u32 = 0xff1b;
const XKB_KEY_DELETE: u32 = 0xffff;
const XKB_KEY_LEFT: u32 = 0xff51;
const XKB_KEY_UP: u32 = 0xff52;
const XKB_KEY_RIGHT: u32 = 0xff53;
const XKB_KEY_DOWN: u32 = 0xff54;
const XKB_KEY_F1: u32 = 0xffbe;
const XKB_KEY_F2: u32 = 0xffbf;
const XKB_KEY_F3: u32 = 0xffc0;
const XKB_KEY_F4: u32 = 0xffc1;
const XKB_KEY_F5: u32 = 0xffc2;
const XKB_KEY_F6: u32 = 0xffc3;
const XKB_KEY_F7: u32 = 0xffc4;
const XKB_KEY_F8: u32 = 0xffc5;
const XKB_KEY_F9: u32 = 0xffc6;
const XKB_KEY_F10: u32 = 0xffc7;
const XKB_KEY_F11: u32 = 0xffc8;
const XKB_KEY_F12: u32 = 0xffc9;

/* ── C ABI types ───────────────────────────────────────────────────────── */

/// A single shortcut binding: modifier mask + optional keysym.
/// If keysym == 0, this is a modifier-only chord (e.g. Ctrl+Shift).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TypioShortcutBinding {
    /// Modifier bitmask (`TYPIO_MOD_*`).
    pub modifiers: u32, /* TYPIO_MOD_* bitmask */
    /// XKB keysym, or 0 for modifier-only chords.
    pub keysym: u32, /* XKB keysym, or 0 for modifier-only */
}

/* ── Default action table ──────────────────────────────────────────────── */

/// Built-in action defaults. Adding a new shortcut means appending one row
/// here plus documenting the action ID — no struct-layout change.
const DEFAULTS: &[(&str, TypioShortcutBinding)] = &[
    (
        "switch_language",
        TypioShortcutBinding {
            modifiers: (TypioModifier::TypioModCtrl as u32) | (TypioModifier::TypioModShift as u32),
            keysym: 0,
        },
    ),
    // "switch_keyboard_engine" has no built-in default since the language
    // model (ADR-0018) took over the Ctrl+Shift chord. Users can still bind
    // it via `shortcuts.switch_keyboard_engine`.
    (
        "exit",
        TypioShortcutBinding {
            modifiers: (TypioModifier::TypioModCtrl as u32) | (TypioModifier::TypioModShift as u32),
            keysym: XKB_KEY_ESCAPE,
        },
    ),
    (
        "voice_ptt",
        TypioShortcutBinding {
            modifiers: TypioModifier::TypioModSuper as u32,
            keysym: b'v' as u32,
        },
    ),
    (
        // Active indicator summon (typio). The on-screen indicator uses
        // a `zwp_input_popup_surface_v2`, so this only fires while a text
        // field is focused; the coordinator's anchor probe recovers a cursor
        // position if the focus edge has no recent rect.
        "summon_indicator",
        TypioShortcutBinding {
            modifiers: (TypioModifier::TypioModCtrl as u32) | (TypioModifier::TypioModSuper as u32),
            keysym: b'i' as u32,
        },
    ),
];

/* ── Modifier name table ───────────────────────────────────────────────── */

const MODIFIER_NAMES: &[(&str, u32)] = &[
    ("ctrl", TypioModifier::TypioModCtrl as u32),
    ("control", TypioModifier::TypioModCtrl as u32),
    ("shift", TypioModifier::TypioModShift as u32),
    ("alt", TypioModifier::TypioModAlt as u32),
    ("super", TypioModifier::TypioModSuper as u32),
];

/* ── Keysym lookup for common names ────────────────────────────────────── */

const KEYSYM_NAMES: &[(&str, u32)] = &[
    ("space", XKB_KEY_SPACE),
    ("return", XKB_KEY_RETURN),
    ("enter", XKB_KEY_RETURN),
    ("tab", XKB_KEY_TAB),
    ("escape", XKB_KEY_ESCAPE),
    ("esc", XKB_KEY_ESCAPE),
    ("backspace", XKB_KEY_BACK_SPACE),
    ("delete", XKB_KEY_DELETE),
    ("up", XKB_KEY_UP),
    ("down", XKB_KEY_DOWN),
    ("left", XKB_KEY_LEFT),
    ("right", XKB_KEY_RIGHT),
    ("f1", XKB_KEY_F1),
    ("f2", XKB_KEY_F2),
    ("f3", XKB_KEY_F3),
    ("f4", XKB_KEY_F4),
    ("f5", XKB_KEY_F5),
    ("f6", XKB_KEY_F6),
    ("f7", XKB_KEY_F7),
    ("f8", XKB_KEY_F8),
    ("f9", XKB_KEY_F9),
    ("f10", XKB_KEY_F10),
    ("f11", XKB_KEY_F11),
    ("f12", XKB_KEY_F12),
];

/* ── Parsing ───────────────────────────────────────────────────────────── */

fn parse_modifier(token: &str) -> u32 {
    for (name, modifier) in MODIFIER_NAMES {
        if token.eq_ignore_ascii_case(name) {
            return *modifier;
        }
    }
    0
}

fn parse_keysym(token: &str) -> u32 {
    for (name, keysym) in KEYSYM_NAMES {
        if token.eq_ignore_ascii_case(name) {
            return *keysym;
        }
    }
    if token.len() == 1 {
        let ch = token.as_bytes()[0].to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            return ch as u32;
        }
    }
    0
}

fn parse_str(s: &str) -> Option<TypioShortcutBinding> {
    if s.is_empty() || s.len() > 128 {
        return None;
    }
    let mut modifiers = 0u32;
    let mut keysym = 0u32;
    let mut token_count = 0;
    for token in s.split('+') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        token_count += 1;
        let m = parse_modifier(token);
        if m != 0 {
            modifiers |= m;
        } else {
            let ks = parse_keysym(token);
            if ks == 0 || keysym != 0 {
                return None;
            }
            keysym = ks;
        }
    }
    if token_count == 0 || modifiers == 0 {
        return None;
    }
    Some(TypioShortcutBinding { modifiers, keysym })
}

/// Parse a shortcut string like "Ctrl+Shift" or "Super+v"
/// into a TypioShortcutBinding. Returns true on success.
#[unsafe(no_mangle)]
pub extern "C" fn typio_shortcut_parse(str: *const c_char, out: *mut TypioShortcutBinding) -> bool {
    if str.is_null() || out.is_null() {
        return false;
    }
    let s = match unsafe { CStr::from_ptr(str) }.to_str() {
        Ok(v) => v,
        Err(_) => return false,
    };
    match parse_str(s) {
        Some(b) => {
            unsafe { *out = b };
            true
        }
        None => false,
    }
}

/* ── Formatting ────────────────────────────────────────────────────────── */

/// Format a binding back to a human-readable string. Caller frees with
/// `typio_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_shortcut_format(binding: *const TypioShortcutBinding) -> *mut c_char {
    if binding.is_null() {
        return ptr::null_mut();
    }
    let b = unsafe { &*binding };

    let mut parts: Vec<String> = Vec::new();
    const ORDER: &[(u32, &str)] = &[
        (TypioModifier::TypioModCtrl as u32, "Ctrl"),
        (TypioModifier::TypioModAlt as u32, "Alt"),
        (TypioModifier::TypioModSuper as u32, "Super"),
        (TypioModifier::TypioModShift as u32, "Shift"),
    ];
    for (mod_val, name) in ORDER {
        if b.modifiers & mod_val != 0 {
            parts.push(name.to_string());
        }
    }

    if b.keysym != 0 {
        let mut found = false;
        for (name, keysym) in KEYSYM_NAMES {
            if *keysym == b.keysym {
                let mut s = name.to_string();
                if let Some(first) = s.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                parts.push(s);
                found = true;
                break;
            }
        }
        if !found {
            let ch = b.keysym as u8;
            if ch.is_ascii_alphanumeric() {
                parts.push(format!("{}", ch as char));
            } else {
                parts.push(format!("0x{:x}", b.keysym));
            }
        }
    }

    let formatted = parts.join("+");
    match CString::new(formatted) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/* ── Action lookup ─────────────────────────────────────────────────────── */

/// Look up the built-in default binding for a named action.
///
/// Standard action IDs: `switch_language`, `exit`, `voice_ptt`.
/// Returns true if the ID is known.
#[unsafe(no_mangle)]
pub extern "C" fn typio_shortcut_default(
    action_id: *const c_char,
    out: *mut TypioShortcutBinding,
) -> bool {
    if action_id.is_null() || out.is_null() {
        return false;
    }
    let id = match unsafe { CStr::from_ptr(action_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    for (name, binding) in DEFAULTS {
        if *name == id {
            unsafe { *out = *binding };
            return true;
        }
    }
    false
}

/// Resolve the binding for a named action, consulting the config and then
/// falling back to the built-in default.
///
/// Reads `shortcuts.<action_id>` from `config`; if missing or unparseable,
/// uses [`typio_shortcut_default`]. Returns true when `out` was written.
#[unsafe(no_mangle)]
pub extern "C" fn typio_shortcut_get(
    config: *const Config,
    action_id: *const c_char,
    out: *mut TypioShortcutBinding,
) -> bool {
    if action_id.is_null() || out.is_null() {
        return false;
    }
    let id = match unsafe { CStr::from_ptr(action_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !config.is_null() {
        let key = format!("shortcuts.{}", id);
        if let Ok(c_key) = CString::new(key) {
            let val = typio_config_get_string(config, c_key.as_ptr(), ptr::null());
            if !val.is_null()
                && let Ok(s) = unsafe { CStr::from_ptr(val) }.to_str()
                && let Some(b) = parse_str(s)
            {
                unsafe { *out = b };
                return true;
            }
        }
    }

    typio_shortcut_default(action_id, out)
}
