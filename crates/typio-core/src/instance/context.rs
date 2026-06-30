//! Input context lifecycle management

use super::TypioInstance;
use crate::c_api::registry::TypioRegistry;
use crate::input_context;
use std::ptr;

/// Get the engine registry owned by this instance.
#[no_mangle]
pub extern "C" fn typio_instance_get_registry(instance: *mut TypioInstance) -> *mut TypioRegistry {
    if instance.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*instance).registry.0 }
}

/// Create a new input context attached to this instance.
#[no_mangle]
pub extern "C" fn typio_instance_create_context(
    instance: *mut TypioInstance,
) -> *mut input_context::TypioInputContext {
    if instance.is_null() {
        return ptr::null_mut();
    }
    let inst = unsafe { &mut *instance };
    let ctx = input_context::typio_input_context_new(instance);
    if ctx.is_null() {
        return ptr::null_mut();
    }
    inst.contexts.push(crate::wrappers::InputContextPtr(ctx));
    ctx
}

/// Destroy an input context and remove it from the instance.
#[no_mangle]
pub extern "C" fn typio_instance_destroy_context(
    instance: *mut TypioInstance,
    ctx: *mut input_context::TypioInputContext,
) {
    if instance.is_null() || ctx.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    inst.contexts.retain(|c| c.0 != ctx);
    if inst.focused_context == ctx {
        inst.focused_context = ptr::null_mut();
    }
    input_context::typio_input_context_free(ctx);
}

/// Get the currently focused input context, or NULL.
#[no_mangle]
pub extern "C" fn typio_instance_get_focused_context(
    instance: *mut TypioInstance,
) -> *mut input_context::TypioInputContext {
    if instance.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*instance).focused_context }
}

/// Set the focused input context.
#[no_mangle]
pub extern "C" fn typio_instance_set_focused_context(
    instance: *mut TypioInstance,
    ctx: *mut input_context::TypioInputContext,
) {
    if instance.is_null() {
        return;
    }
    unsafe {
        (*instance).focused_context = ctx;
    }
}
