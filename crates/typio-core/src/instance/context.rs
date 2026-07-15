//! Input context lifecycle management

use super::TypioInstance;
use crate::c_api::registry::TypioRegistry;
use crate::input_context;
use std::ptr;

/// Get the engine registry owned by this instance.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_get_registry(instance: *mut TypioInstance) -> *mut TypioRegistry {
    if instance.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*instance).registry.0 }
}

/// Create a new input context attached to this instance.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_destroy_context(
    instance: *mut TypioInstance,
    ctx: *mut input_context::TypioInputContext,
) {
    if instance.is_null() || ctx.is_null() {
        return;
    }
    let inst = unsafe { &mut *instance };
    let Some(index) = inst
        .contexts
        .iter()
        .position(|candidate| candidate.0 == ctx)
    else {
        return;
    };
    if inst.focused_context == ctx {
        inst.focused_context = ptr::null_mut();
    }
    // InputContextPtr owns the allocation. Removing it drops the context once;
    // an explicit free here would double-free the same Box.
    inst.contexts.swap_remove(index);
}

/// Get the currently focused input context, or NULL.
#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_get_focused_context(
    instance: *mut TypioInstance,
) -> *mut input_context::TypioInputContext {
    if instance.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*instance).focused_context }
}

/// Set the focused input context.
#[unsafe(no_mangle)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destroy_context_removes_and_frees_the_owned_context_once() {
        let instance = crate::instance::typio_instance_new();
        assert!(!instance.is_null());
        let ctx = typio_instance_create_context(instance);
        assert!(!ctx.is_null());
        typio_instance_set_focused_context(instance, ctx);

        typio_instance_destroy_context(instance, ctx);

        let inst = unsafe { &*instance };
        assert!(inst.contexts.is_empty());
        assert!(inst.focused_context.is_null());
        crate::instance::typio_instance_free(instance);
    }
}
