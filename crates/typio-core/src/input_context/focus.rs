//! Focus management and key event forwarding

use super::TypioInputContext;
use crate::TypioKeyEvent;
use crate::instance::{
    TypioInstance, dispatch_observed_keyboard_mode, typio_instance_get_registry,
    typio_instance_set_focused_context,
};
use crate::types::*;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

/// Drain any keyboard-mode change the process backend observed in the last reply
/// and forward it to the host.
///
/// Out-of-process keyboard engines report their active mode in the reply to
/// every mode-affecting request, so this is called right after each one.
/// `announce` selects the host path: `true` for the **deliberate** change
/// driven by user input (`process-key`), `false` for **incidental**
/// host-driven requests (focus, reset, mode restore) that should refresh state
/// without an unprompted confirmation.
///
/// # Safety
/// `instance` must be a valid `TypioInstance` pointer.
unsafe fn reconcile_keyboard_mode(instance: *mut TypioInstance, announce: bool) {
    let registry = typio_instance_get_registry(instance);
    if registry.is_null() {
        return;
    }
    if let Some(mode) = (*registry).inner.take_active_keyboard_changed_mode() {
        dispatch_observed_keyboard_mode(instance, &mode, announce);
    }
}

/// Notify the context that it has received focus.
#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_focus_in(ctx: *mut TypioInputContext) {
    if ctx.is_null() {
        return;
    }
    let ctx_ref = unsafe { &mut *ctx };

    if ctx_ref.focused {
        return;
    }
    ctx_ref.focused = true;

    typio_instance_set_focused_context(ctx_ref.instance, ctx);
    unsafe {
        let registry = typio_instance_get_registry(ctx_ref.instance);
        if !registry.is_null() {
            let mut ictx = crate::core::engine::InputContext::from_raw(ctx);
            (*registry).inner.focus_in_active_keyboard(&mut ictx);
        }
        reconcile_keyboard_mode(ctx_ref.instance, false);
    }
}

/// Notify the context that it has lost focus.
#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_focus_out(ctx: *mut TypioInputContext) {
    if ctx.is_null() {
        return;
    }
    let ctx_ref = unsafe { &mut *ctx };

    if !ctx_ref.focused {
        return;
    }

    unsafe {
        let registry = typio_instance_get_registry(ctx_ref.instance);
        if !registry.is_null() {
            let mut ictx = crate::core::engine::InputContext::from_raw(ctx);
            (*registry).inner.focus_out_active_keyboard(&mut ictx);
        }
        typio_instance_set_focused_context(ctx_ref.instance, ptr::null_mut());
    }

    ctx_ref.focused = false;
}

/// Return true if the context currently has focus.
#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_is_focused(ctx: *mut TypioInputContext) -> bool {
    if ctx.is_null() {
        return false;
    }
    unsafe { (*ctx).focused }
}

/// Reset the context (clear composition and notify engine).
#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_reset(ctx: *mut TypioInputContext) {
    if ctx.is_null() {
        return;
    }

    super::typio_input_context_clear(ctx);

    unsafe {
        let registry = typio_instance_get_registry((*ctx).instance);
        if !registry.is_null() {
            let mut ictx = crate::core::engine::InputContext::from_raw(ctx);
            (*registry).inner.reset_active_keyboard(&mut ictx);
        }
        reconcile_keyboard_mode((*ctx).instance, false);
    }
}

/// Forward a key event to the active keyboard engine.
#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_process_key(
    ctx: *mut TypioInputContext,
    event: *const TypioKeyEvent,
) -> bool {
    if ctx.is_null() || event.is_null() {
        return false;
    }

    let result = unsafe {
        let registry = typio_instance_get_registry((*ctx).instance);
        if registry.is_null() {
            crate::core::engine::KeyProcessResult::NotHandled
        } else {
            let c_event = &*event;
            let mut ictx = crate::core::engine::InputContext::from_raw(ctx);
            let r_event = crate::core::engine::KeyEvent {
                sym: crate::core::engine::KeySym::Raw(c_event.keysym),
                state: match c_event.type_ {
                    TypioEventType::TypioEventKeyRelease => crate::core::engine::KeyState::Release,
                    _ => crate::core::engine::KeyState::Press,
                },
                code: c_event.keycode,
                modifiers: c_event.modifiers,
                unicode: c_event.unicode,
                time: c_event.time,
                is_repeat: c_event.is_repeat,
                base_keysym: if c_event.struct_size >= std::mem::size_of::<TypioKeyEvent>() {
                    c_event.base_keysym
                } else {
                    0
                },
            };
            let outcome = (*registry)
                .inner
                .process_key_active_keyboard(&mut ictx, &r_event);
            // A keystroke is a deliberate user action: announce any resulting
            // mode change so the host confirms it unconditionally.
            reconcile_keyboard_mode((*ctx).instance, true);
            outcome
        }
    };

    result != crate::core::engine::KeyProcessResult::NotHandled
}

/// Set the active keyboard engine's mode for this context.
///
/// The host calls this to restore a remembered mode on focus.
/// `mode_id` is a previously reported `TypioKeyboardEngineMode::id`.
/// Returns `TypioErrorNotFound` when there is no active keyboard, the engine
/// has no `set_active_mode`, or the engine rejects the id.
#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_set_active_mode(
    ctx: *mut TypioInputContext,
    mode_id: *const c_char,
) -> TypioResult {
    if ctx.is_null() || mode_id.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let id = match unsafe { CStr::from_ptr(mode_id) }.to_str() {
        Ok(s) if !s.is_empty() => s,
        _ => return TypioResult::TypioErrorInvalidArgument,
    };
    unsafe {
        let registry = typio_instance_get_registry((*ctx).instance);
        if registry.is_null() {
            return TypioResult::TypioErrorNotFound;
        }
        let mut ictx = crate::core::engine::InputContext::from_raw(ctx);
        let outcome = (*registry).inner.set_active_mode_keyboard(&mut ictx, id);
        // Host-initiated restore/switch: refresh state without an unprompted
        // confirmation — the host's focus path applies its own salience gate.
        reconcile_keyboard_mode((*ctx).instance, false);
        match outcome {
            Ok(()) => TypioResult::TypioOk,
            Err(_) => TypioResult::TypioErrorNotFound,
        }
    }
}

/// Commit a candidate selected by the host (ADR-0012).
///
/// Dispatches `commit_candidate` to the active keyboard engine.
/// Returns `TypioErrorNotFound` when no keyboard engine is active or the
/// engine does not implement `commit_candidate`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_commit_candidate(
    ctx: *mut TypioInputContext,
    candidate_index: i32,
) -> TypioResult {
    if ctx.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    unsafe {
        let registry = typio_instance_get_registry((*ctx).instance);
        if registry.is_null() {
            return TypioResult::TypioErrorNotFound;
        }
        let mut ictx = crate::core::engine::InputContext::from_raw(ctx);
        match (*registry)
            .inner
            .commit_candidate_active_keyboard(&mut ictx, candidate_index)
        {
            Ok(()) => TypioResult::TypioOk,
            Err(crate::core::engine::EngineError::NotSupported) => TypioResult::TypioErrorNotFound,
            Err(_) => TypioResult::TypioErrorNotFound,
        }
    }
}
