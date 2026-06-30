//! Minimal keyboard engine using typio-vet for ABI + tests.
//!
//! Run the tests:
//!   cargo test --example demo_engine
//!
//! This file demonstrates the Rust dev-dependency test pattern. Cargo examples
//! are binaries, not cdylibs, so to vet it through the CLI you would build the
//! same source as a `crate-type = ["cdylib"]` library and point `typio-vet` at
//! the resulting `.so`.

use std::ffi::{c_void, CString};
use std::ptr;
use typio_vet::*;

/* -------------------------------------------------------------------------- */
/* Engine implementation                                                      */
/* -------------------------------------------------------------------------- */

struct DemoData;

extern "C" fn demo_init(engine: *mut TypioEngine, _instance: *mut TypioInstance) -> TypioResult {
    if engine.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    unsafe {
        let data = Box::new(DemoData);
        (*engine).user_data = Box::into_raw(data) as *mut c_void;
    }
    TypioResult::TypioOk
}

extern "C" fn demo_destroy(engine: *mut TypioEngine) {
    if engine.is_null() {
        return;
    }
    unsafe {
        if !(*engine).user_data.is_null() {
            drop(Box::from_raw((*engine).user_data as *mut DemoData));
            (*engine).user_data = ptr::null_mut();
        }
    }
}

extern "C" fn demo_process_key(
    engine: *mut TypioKeyboardEngine,
    ctx: *mut TypioInputContext,
    event: *const c_void,
) -> TypioKeyProcessResult {
    if engine.is_null() || event.is_null() {
        return TypioKeyProcessResult::TypioKeyNotHandled;
    }
    let ev = unsafe { &*(event as *const TypioKeyEvent) };
    if ev.type_ != TypioEventType::TypioEventKeyPress {
        return TypioKeyProcessResult::TypioKeyNotHandled;
    }
    if ev.unicode < 0x20 || ev.unicode == 0x7F {
        return TypioKeyProcessResult::TypioKeyNotHandled;
    }
    let c = char::from_u32(ev.unicode).unwrap_or('?');
    let text = CString::new(c.to_string()).unwrap();
    typio_input_context_commit(ctx, text.as_ptr());
    std::mem::forget(text);
    TypioKeyProcessResult::TypioKeyCommitted
}

extern "C" fn demo_nop(_engine: *mut TypioEngine) {}
extern "C" fn demo_nop_ctx(_engine: *mut TypioEngine, _ctx: *mut TypioInputContext) {}
extern "C" fn demo_reload_ok(_engine: *mut TypioEngine) -> TypioResult {
    TypioResult::TypioOk
}

static DEMO_BASE_OPS: TypioEngineBaseOps = TypioEngineBaseOps {
    init: Some(demo_init),
    destroy: Some(demo_destroy),
    deactivate: Some(demo_nop),
    focus_in: Some(demo_nop_ctx),
    focus_out: Some(demo_nop_ctx),
    reset: Some(demo_nop_ctx),
    reload_config: Some(demo_reload_ok),
    on_config_change: None,
    availability: None,
};

static DEMO_KEYBOARD_OPS: TypioKeyboardEngineOps = TypioKeyboardEngineOps {
    process_key: Some(demo_process_key),
    list_modes: None,
    get_active_mode: None,
    set_active_mode: None,
    commit_candidate: None,
};

static DEMO_INFO: TypioEngineInfo = TypioEngineInfo {
    name: c"demo".as_ptr(),
    display_name: c"Demo".as_ptr(),
    description: c"Example engine for testing".as_ptr(),
    author: c"Typio".as_ptr(),
    icon: ptr::null(),
    language: c"und".as_ptr(),
    type_: TypioEngineType::TypioEngineTypeKeyboard,
    required_capabilities: ptr::null(),
    optional_capabilities: ptr::null(),
};

#[no_mangle]
pub extern "C" fn typio_engine_get_info() -> *const TypioEngineInfo {
    &DEMO_INFO
}

#[no_mangle]
pub extern "C" fn typio_keyboard_engine_create() -> *mut TypioKeyboardEngine {
    let engine = unsafe {
        libc::calloc(1, std::mem::size_of::<TypioKeyboardEngine>()) as *mut TypioKeyboardEngine
    };
    if engine.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*engine).base.info = &DEMO_INFO;
        (*engine).base.base_ops = &DEMO_BASE_OPS;
        (*engine).keyboard = &DEMO_KEYBOARD_OPS;
    }
    engine
}

/* -------------------------------------------------------------------------- */
/* Tests — no type replication, no pointer casts                              */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_commits_printable_key() {
        let mut harness =
            unsafe { TestHarness::new_keyboard(typio_keyboard_engine_create, Default::default()) }
                .expect("init failed");

        let result = unsafe { harness.press(&key_press('a')) };
        assert_eq!(result, TypioKeyProcessResult::TypioKeyCommitted);
        assert_eq!(harness.log.take(), vec![ContextEvent::Commit("a".into())]);

        unsafe { harness.destroy() };
    }

    #[test]
    fn demo_passthrough_non_printable() {
        let mut harness =
            unsafe { TestHarness::new_keyboard(typio_keyboard_engine_create, Default::default()) }
                .expect("init failed");

        let ev = TypioKeyEvent {
            struct_size: std::mem::size_of::<TypioKeyEvent>(),
            type_: TypioEventType::TypioEventKeyPress,
            keycode: 0,
            keysym: 0xFF1B, // Escape
            modifiers: 0,
            unicode: 0x1B,
            time: 0,
            is_repeat: false,
        };
        let result = unsafe { harness.press(&ev) };
        assert_eq!(result, TypioKeyProcessResult::TypioKeyNotHandled);
        assert!(harness.log.take().is_empty());

        unsafe { harness.destroy() };
    }

    #[test]
    fn demo_lifecycle_survives_focus_churn() {
        let mut harness =
            unsafe { TestHarness::new_keyboard(typio_keyboard_engine_create, Default::default()) }
                .expect("init failed");

        unsafe {
            harness.focus_in();
            harness.focus_out();
            harness.focus_in();
        }

        let result = unsafe { harness.press(&key_press('z')) };
        assert_eq!(result, TypioKeyProcessResult::TypioKeyCommitted);
        assert_eq!(harness.log.take(), vec![ContextEvent::Commit("z".into())]);

        unsafe { harness.destroy() };
    }
}

fn main() {
    println!("This is a demo engine. Run the tests with: cargo test --example demo_engine");
}
