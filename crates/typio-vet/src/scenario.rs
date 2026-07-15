//! ABI and behavioral checks driven against a loaded engine.
//!
//! Behavioral checks create a *fresh* engine instance per scenario so that
//! state from one scenario cannot leak into the next. Every assertion here is
//! an invariant that holds for **any** conforming engine — anything an engine
//! is merely *likely* (but not required) to do is reported as `Warn`, never
//! `Fail`, so the gate stays free of false positives.

use std::ffi::{CStr, c_void};

use typio_abi::*;

use crate::check::{CheckCategory, CheckResult};
use crate::mock::*;

const ABI: CheckCategory = CheckCategory::Abi;
const BEHAVIOR: CheckCategory = CheckCategory::Behavior;

pub type KeyboardFactory = unsafe extern "C" fn() -> *mut TypioKeyboardEngine;
pub type VoiceFactory = unsafe extern "C" fn() -> *mut TypioVoiceEngine;

/* -------------------------------------------------------------------------- */
/* Shared ABI: TypioEngineInfo                                                */
/* -------------------------------------------------------------------------- */

/// Validate the engine's `TypioEngineInfo` against the slot it was loaded as.
///
/// # Safety
/// `info` must be a valid pointer (or null) returned by the engine.
pub unsafe fn info_checks(
    info: *const TypioEngineInfo,
    expected: TypioEngineType,
) -> Vec<CheckResult> {
    let mut out = Vec::new();

    if info.is_null() {
        out.push(CheckResult::fail(
            ABI,
            "info_present",
            "TypioEngineInfo is null",
        ));
        return out;
    }
    out.push(CheckResult::pass(ABI, "info_present"));
    let info = &*info;

    // name (required)
    out.push(match cstr(info.name) {
        Some(s) if !s.is_empty() => CheckResult::pass(ABI, "name"),
        Some(_) => CheckResult::fail(ABI, "name", "name is empty"),
        None => CheckResult::fail(ABI, "name", "name is null or not valid UTF-8"),
    });

    // display_name / author / language (recommended)
    for (field, ptr) in [
        ("display_name", info.display_name),
        ("author", info.author),
        ("language", info.language),
    ] {
        out.push(match cstr(ptr) {
            Some(s) if !s.is_empty() => CheckResult::pass(ABI, leak_name(field)),
            _ => CheckResult::warn(ABI, leak_name(field), format!("{field} is missing")),
        });
    }

    // type matches the loaded slot
    out.push(if info.type_ as u32 == expected as u32 {
        CheckResult::pass(ABI, "type_matches_slot")
    } else {
        CheckResult::fail(
            ABI,
            "type_matches_slot",
            format!("expected {:?}, info reports {:?}", expected, info.type_),
        )
    });

    out
}

/* -------------------------------------------------------------------------- */
/* Keyboard                                                                   */
/* -------------------------------------------------------------------------- */

/// Run the full keyboard check suite (ABI + behavior).
///
/// # Safety
/// `create` must be the engine's real `typio_keyboard_engine_create` export.
pub unsafe fn keyboard_checks(create: KeyboardFactory) -> Vec<CheckResult> {
    let mut out = Vec::new();

    // Inspect one engine for ABI/vtable structure (no init required).
    let probe = create();
    if probe.is_null() {
        out.push(CheckResult::fail(ABI, "create", "factory returned null"));
        return out;
    }
    out.push(CheckResult::pass(ABI, "create"));

    out.extend(info_checks(
        (*probe).base.info,
        TypioEngineType::TypioEngineTypeKeyboard,
    ));
    out.extend(base_vtable_checks(&(*probe).base));

    // keyboard vtable
    let kb = (*probe).keyboard;
    if kb.is_null() {
        out.push(CheckResult::fail(
            ABI,
            "keyboard_vtable",
            "keyboard ops pointer is null",
        ));
    } else {
        out.push(CheckResult::pass(ABI, "keyboard_vtable"));
        out.push(if (*kb).process_key.is_some() {
            CheckResult::pass(ABI, "process_key_present")
        } else {
            CheckResult::fail(
                ABI,
                "process_key_present",
                "keyboard engine has no process_key",
            )
        });
    }
    libc::free(probe as *mut c_void); // never initialized; safe to release

    // Behavior — each scenario gets a fresh, freshly-initialized engine.
    out.push(kb_lifecycle(create));
    out.extend(kb_drive(create));

    out
}

unsafe fn kb_lifecycle(create: KeyboardFactory) -> CheckResult {
    let engine = create();
    if engine.is_null() {
        return CheckResult::fail(BEHAVIOR, "lifecycle", "factory returned null");
    }
    let base = &mut (*engine).base;
    if let Some(init) = (*base.base_ops).init {
        let inst = mock_instance(Default::default());
        let r = init(base, inst);
        if r != TypioResult::TypioOk {
            free_instance(inst);
            libc::free(engine as *mut c_void);
            return CheckResult::fail(BEHAVIOR, "lifecycle", format!("init returned {:?}", r));
        }
        if let Some(destroy) = (*base.base_ops).destroy {
            destroy(base);
        }
        free_instance(inst);
    }
    libc::free(engine as *mut c_void);
    CheckResult::pass(BEHAVIOR, "lifecycle")
}

/// The scenarios that need a live, initialized engine.
unsafe fn kb_drive(create: KeyboardFactory) -> Vec<CheckResult> {
    let mut out = Vec::new();

    macro_rules! harness_or_skip {
        ($name:expr) => {
            match TestHarness::new_keyboard(create, Default::default()) {
                Some(h) => h,
                None => {
                    out.push(CheckResult::fail(BEHAVIOR, $name, "engine init failed"));
                    return out;
                }
            }
        };
    }

    // printable_key: result code must cohere with side effects.
    {
        let mut h = harness_or_skip!("printable_key");
        let r = h.press(&key_press('a'));
        let events = h.log.take();
        let commits: Vec<&String> = events
            .iter()
            .filter_map(|e| match e {
                ContextEvent::Commit(s) => Some(s),
                _ => None,
            })
            .collect();
        out.push(match r {
            TypioKeyProcessResult::TypioKeyCommitted => {
                if commits.is_empty() {
                    CheckResult::fail(BEHAVIOR, "printable_key", "returned COMMITTED but emitted no commit")
                } else if commits.iter().all(|s| s.is_empty()) {
                    CheckResult::fail(BEHAVIOR, "printable_key", "committed an empty string")
                } else {
                    CheckResult::pass(BEHAVIOR, "printable_key")
                }
            }
            TypioKeyProcessResult::TypioKeyNotHandled => {
                if !events.is_empty() {
                    CheckResult::fail(BEHAVIOR, "printable_key", "returned NOT_HANDLED but emitted context events")
                } else {
                    CheckResult::warn(
                        BEHAVIOR,
                        "printable_key",
                        "declined plain key 'a' (expected for passthrough/Latin engines, suspicious otherwise)",
                    )
                }
            }
            TypioKeyProcessResult::TypioKeyComposing => {
                if !commits.is_empty() {
                    CheckResult::fail(BEHAVIOR, "printable_key", "returned COMPOSING but also committed")
                } else {
                    CheckResult::pass(BEHAVIOR, "printable_key")
                }
            }
            TypioKeyProcessResult::TypioKeyHandled => CheckResult::pass(BEHAVIOR, "printable_key"),
        });
        unsafe { h.destroy() };
    }

    // modifier_passthrough: a lone modifier must never commit.
    {
        let mut h = harness_or_skip!("modifier_passthrough");
        let r = h.press(&modifier_key(TYPIO_KEY_Shift_L));
        let events = h.log.take();
        let committed = matches!(r, TypioKeyProcessResult::TypioKeyCommitted)
            || events.iter().any(|e| matches!(e, ContextEvent::Commit(_)));
        out.push(if committed {
            CheckResult::fail(
                BEHAVIOR,
                "modifier_passthrough",
                format!("lone modifier key produced a commit (result={:?})", r),
            )
        } else {
            CheckResult::pass(BEHAVIOR, "modifier_passthrough")
        });
        unsafe { h.destroy() };
    }

    // escape_on_empty: Escape on an empty context must not commit text.
    {
        let mut h = harness_or_skip!("escape_on_empty");
        let _ = h.press(&escape_key());
        let events = h.log.take();
        out.push(
            if events.iter().any(|e| matches!(e, ContextEvent::Commit(_))) {
                CheckResult::fail(
                    BEHAVIOR,
                    "escape_on_empty",
                    "Escape on empty context committed text",
                )
            } else {
                CheckResult::pass(BEHAVIOR, "escape_on_empty")
            },
        );
        unsafe { h.destroy() };
    }

    // focus_churn: in -> out -> in, then the engine must still respond.
    {
        let mut h = harness_or_skip!("focus_churn");
        h.focus_in();
        h.focus_out();
        h.focus_in();
        let _ = h.press(&key_press('a')); // survival is the assertion
        let _ = h.log.take();
        out.push(CheckResult::pass(BEHAVIOR, "focus_churn"));
        unsafe { h.destroy() };
    }

    // reset_clears: reset must abort composition, never commit.
    {
        let mut h = harness_or_skip!("reset");
        let _ = h.press(&key_press('a'));
        let _ = h.log.take();
        h.reset();
        let events = h.log.take();
        out.push(
            if events.iter().any(|e| matches!(e, ContextEvent::Commit(_))) {
                CheckResult::fail(
                    BEHAVIOR,
                    "reset",
                    "reset emitted a commit instead of clearing",
                )
            } else {
                CheckResult::pass(BEHAVIOR, "reset")
            },
        );
        unsafe { h.destroy() };
    }

    // config_reload: must succeed if the engine implements it.
    {
        let mut h = harness_or_skip!("config_reload");
        let r = h.reload_config();
        out.push(if r == TypioResult::TypioOk {
            CheckResult::pass(BEHAVIOR, "config_reload")
        } else {
            CheckResult::fail(
                BEHAVIOR,
                "config_reload",
                format!("reload_config returned {:?}", r),
            )
        });
        unsafe { h.destroy() };
    }

    out
}

/* -------------------------------------------------------------------------- */
/* Voice                                                                      */
/* -------------------------------------------------------------------------- */

/// Run the full voice check suite (ABI + behavior).
///
/// # Safety
/// `create` must be the engine's real `typio_voice_engine_create` export.
pub unsafe fn voice_checks(create: VoiceFactory) -> Vec<CheckResult> {
    let mut out = Vec::new();

    let probe = create();
    if probe.is_null() {
        out.push(CheckResult::fail(ABI, "create", "factory returned null"));
        return out;
    }
    out.push(CheckResult::pass(ABI, "create"));

    out.extend(info_checks(
        (*probe).base.info,
        TypioEngineType::TypioEngineTypeVoice,
    ));
    out.extend(base_vtable_checks(&(*probe).base));

    let voice = (*probe).voice;
    if voice.is_null() {
        out.push(CheckResult::fail(
            ABI,
            "voice_vtable",
            "voice ops pointer is null",
        ));
    } else {
        out.push(CheckResult::pass(ABI, "voice_vtable"));
        out.push(if (*voice).process_audio.is_some() {
            CheckResult::pass(ABI, "process_audio_present")
        } else {
            CheckResult::fail(
                ABI,
                "process_audio_present",
                "voice engine has no process_audio",
            )
        });
    }
    libc::free(probe as *mut c_void);

    out.push(voice_lifecycle(create));
    out.extend(voice_drive(create));
    out
}

unsafe fn voice_lifecycle(create: VoiceFactory) -> CheckResult {
    let engine = create();
    if engine.is_null() {
        return CheckResult::fail(BEHAVIOR, "lifecycle", "factory returned null");
    }
    let base = &mut (*engine).base;
    if let Some(init) = (*base.base_ops).init {
        let inst = mock_instance(Default::default());
        let r = init(base, inst);
        if r != TypioResult::TypioOk {
            free_instance(inst);
            libc::free(engine as *mut c_void);
            return CheckResult::fail(BEHAVIOR, "lifecycle", format!("init returned {:?}", r));
        }
        if let Some(destroy) = (*base.base_ops).destroy {
            destroy(base);
        }
        free_instance(inst);
    }
    libc::free(engine as *mut c_void);
    CheckResult::pass(BEHAVIOR, "lifecycle")
}

unsafe fn voice_drive(create: VoiceFactory) -> Vec<CheckResult> {
    let mut out = Vec::new();

    let engine = create();
    if engine.is_null() {
        out.push(CheckResult::fail(
            BEHAVIOR,
            "process_audio_silent",
            "factory returned null",
        ));
        return out;
    }
    let base = &mut (*engine).base;
    let inst = mock_instance(Default::default());
    let inited = match (*base.base_ops).init {
        Some(init) => init(base, inst) == TypioResult::TypioOk,
        None => true,
    };
    if !inited {
        free_instance(inst);
        libc::free(engine as *mut c_void);
        out.push(CheckResult::fail(
            BEHAVIOR,
            "process_audio_silent",
            "engine init failed",
        ));
        return out;
    }

    let voice = (*engine).voice;

    // process_audio over 1s of silence: must not crash; any returned text must
    // be a valid NUL-terminated UTF-8 C string.
    out.push(if voice.is_null() || (*voice).process_audio.is_none() {
        CheckResult::fail(BEHAVIOR, "process_audio_silent", "process_audio missing")
    } else {
        let silence = vec![0.0f32; 16_000];
        let text = ((*voice).process_audio.unwrap())(engine, silence.as_ptr(), silence.len());
        if text.is_null() {
            CheckResult::pass(BEHAVIOR, "process_audio_silent")
        } else {
            let ok = CStr::from_ptr(text).to_str().is_ok();
            let v = if ok {
                CheckResult::pass(BEHAVIOR, "process_audio_silent")
            } else {
                CheckResult::fail(
                    BEHAVIOR,
                    "process_audio_silent",
                    "returned text is not valid UTF-8",
                )
            };
            libc::free(text as *mut c_void);
            v
        }
    });

    if let Some(destroy) = (*base.base_ops).destroy {
        destroy(base);
    }
    free_instance(inst);
    libc::free(engine as *mut c_void);
    out
}

/* -------------------------------------------------------------------------- */
/* Helpers                                                                    */
/* -------------------------------------------------------------------------- */

/// Checks on the base vtable shared by all engine kinds.
unsafe fn base_vtable_checks(base: &TypioEngine) -> Vec<CheckResult> {
    let mut out = Vec::new();
    if base.base_ops.is_null() {
        out.push(CheckResult::fail(
            ABI,
            "base_vtable",
            "base_ops pointer is null",
        ));
        return out;
    }
    out.push(CheckResult::pass(ABI, "base_vtable"));
    let ops = &*base.base_ops;
    out.push(if ops.init.is_some() {
        CheckResult::pass(ABI, "init_present")
    } else {
        CheckResult::warn(
            ABI,
            "init_present",
            "no init op; host cannot configure the engine",
        )
    });
    out.push(if ops.destroy.is_some() {
        CheckResult::pass(ABI, "destroy_present")
    } else {
        CheckResult::warn(
            ABI,
            "destroy_present",
            "no destroy op; engine resources may leak",
        )
    });
    out
}

fn cstr(p: *const std::ffi::c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

/// Check names are `&'static str`; field names come from a fixed set, so we can
/// safely intern them to satisfy that bound.
fn leak_name(field: &str) -> &'static str {
    match field {
        "display_name" => "display_name",
        "author" => "author",
        "language" => "language",
        _ => "info_field",
    }
}
