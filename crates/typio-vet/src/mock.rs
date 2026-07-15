//! Mock libtypio host.
//!
//! The functions in this module are exported with `#[unsafe(no_mangle)]` so that an
//! native engine artifact loaded via `dlopen` resolves its `typio_*` host
//! imports against `typio-vet` instead of the real `libtypio`. Each call is
//! recorded so scenarios can assert on what the engine actually did.
//!
//! The `#[unsafe(no_mangle)] extern "C"` exports below dereference raw pointers by
//! design: they must mirror the real `libtypio` C ABI byte-for-byte, which the
//! engine calls across FFI. The Rust `unsafe` keyword is invisible to a C
//! caller, so marking them `unsafe` would buy no safety while diverging from
//! the contract they reimplement. The `TestHarness` methods are `unsafe`
//! because the caller owns the engine-pointer validity documented on the type.
#![allow(clippy::not_unsafe_ptr_arg_deref, clippy::missing_safety_doc)]

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::{LazyLock, Mutex};

use typio_abi::*;

/* -------------------------------------------------------------------------- */
/* Recorded host effects                                                      */
/* -------------------------------------------------------------------------- */

/// An observable side effect an engine produced on a context.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextEvent {
    Commit(String),
    Clear,
    SetComposition {
        preedit: String,
        cursor: i32,
        candidate_count: usize,
    },
}

/// A config value the mock host hands back to the engine.
#[derive(Debug, Clone)]
pub enum ConfigValue {
    Bool(bool),
    String(String),
    Int(i64),
    Float(f64),
}

static CONTEXT_LOGS: LazyLock<Mutex<HashMap<u64, Vec<ContextEvent>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CONFIGS: LazyLock<Mutex<HashMap<u64, HashMap<String, ConfigValue>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: Mutex<u64> = Mutex::new(1);

fn next_id() -> u64 {
    let mut g = NEXT_ID.lock().unwrap();
    let id = *g;
    *g += 1;
    id
}

/// Drain and return the event log for a context.
pub fn drain_log(ctx_id: u64) -> Vec<ContextEvent> {
    CONTEXT_LOGS
        .lock()
        .unwrap()
        .remove(&ctx_id)
        .unwrap_or_default()
}

/// Peek at the event log for a context without draining.
pub fn peek_log(ctx_id: u64) -> Vec<ContextEvent> {
    CONTEXT_LOGS
        .lock()
        .unwrap()
        .get(&ctx_id)
        .cloned()
        .unwrap_or_default()
}

/* -------------------------------------------------------------------------- */
/* Mock ABI implementations                                                   */
/* -------------------------------------------------------------------------- */

#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_commit(ctx: *mut TypioInputContext, text: *const c_char) {
    if ctx.is_null() || text.is_null() {
        return;
    }
    let id = unsafe { *(ctx as *mut u64) };
    let s = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    CONTEXT_LOGS
        .lock()
        .unwrap()
        .entry(id)
        .or_default()
        .push(ContextEvent::Commit(s));
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_clear(ctx: *mut TypioInputContext) {
    if ctx.is_null() {
        return;
    }
    let id = unsafe { *(ctx as *mut u64) };
    CONTEXT_LOGS
        .lock()
        .unwrap()
        .entry(id)
        .or_default()
        .push(ContextEvent::Clear);
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_input_context_set_composition(
    ctx: *mut TypioInputContext,
    comp: *const TypioComposition,
) {
    if ctx.is_null() || comp.is_null() {
        return;
    }
    let id = unsafe { *(ctx as *mut u64) };
    let comp = unsafe { &*comp };

    let preedit = if comp.segment_count > 0 && !comp.segments.is_null() {
        let seg = unsafe { &*comp.segments };
        if seg.text.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(seg.text) }
                .to_string_lossy()
                .into_owned()
        }
    } else {
        String::new()
    };

    CONTEXT_LOGS
        .lock()
        .unwrap()
        .entry(id)
        .or_default()
        .push(ContextEvent::SetComposition {
            preedit,
            cursor: comp.cursor_pos,
            candidate_count: comp.candidate_count,
        });
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_get_config(instance: *mut TypioInstance) -> *mut c_void {
    if instance.is_null() {
        return std::ptr::null_mut();
    }
    instance as *mut c_void
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_get_engine_config(
    instance: *mut TypioInstance,
    _name: *const c_char,
) -> *mut c_void {
    typio_instance_get_config(instance)
}

fn inst_id_from_config(config: *mut c_void) -> u64 {
    unsafe { *(config as *mut u64) }
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_config_get_bool(
    config: *mut c_void,
    key: *const c_char,
    default: bool,
) -> bool {
    if config.is_null() || key.is_null() {
        return default;
    }
    let key = unsafe { CStr::from_ptr(key) }
        .to_string_lossy()
        .into_owned();
    let inst_id = inst_id_from_config(config);
    let configs = CONFIGS.lock().unwrap();
    if let Some(ConfigValue::Bool(v)) = configs.get(&inst_id).and_then(|m| m.get(&key)) {
        return *v;
    }
    default
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_config_get_string(
    config: *mut c_void,
    key: *const c_char,
    default: *const c_char,
) -> *const c_char {
    if config.is_null() || key.is_null() {
        return default;
    }
    let key = unsafe { CStr::from_ptr(key) }
        .to_string_lossy()
        .into_owned();
    let inst_id = inst_id_from_config(config);
    let configs = CONFIGS.lock().unwrap();
    if let Some(ConfigValue::String(v)) = configs.get(&inst_id).and_then(|m| m.get(&key)) {
        return CString::new(v.clone()).unwrap().into_raw();
    }
    default
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_config_get_int(
    config: *mut c_void,
    key: *const c_char,
    default: i64,
) -> i64 {
    if config.is_null() || key.is_null() {
        return default;
    }
    let key = unsafe { CStr::from_ptr(key) }
        .to_string_lossy()
        .into_owned();
    let inst_id = inst_id_from_config(config);
    let configs = CONFIGS.lock().unwrap();
    if let Some(ConfigValue::Int(v)) = configs.get(&inst_id).and_then(|m| m.get(&key)) {
        return *v;
    }
    default
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_config_get_float(
    config: *mut c_void,
    key: *const c_char,
    default: f64,
) -> f64 {
    if config.is_null() || key.is_null() {
        return default;
    }
    let key = unsafe { CStr::from_ptr(key) }
        .to_string_lossy()
        .into_owned();
    let inst_id = inst_id_from_config(config);
    let configs = CONFIGS.lock().unwrap();
    if let Some(ConfigValue::Float(v)) = configs.get(&inst_id).and_then(|m| m.get(&key)) {
        return *v;
    }
    default
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_key_event_is_modifier_only(event: *const TypioKeyEvent) -> bool {
    if event.is_null() {
        return false;
    }
    let e = unsafe { &*event };
    // Keysym range for modifiers: Shift_L ..= Hyper_R.
    (0xFFE1..=0xFFEE).contains(&e.keysym)
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_key_event_is_escape(event: *const TypioKeyEvent) -> bool {
    if event.is_null() {
        return false;
    }
    unsafe { &*event }.keysym == TYPIO_KEY_Escape
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_notify_keyboard_mode(
    _instance: *mut TypioInstance,
    _mode: *const TypioKeyboardEngineMode,
) {
}

#[unsafe(no_mangle)]
pub extern "C" fn typio_instance_clear_keyboard_mode(_instance: *mut TypioInstance) {}

/* -------------------------------------------------------------------------- */
/* Context / instance fixtures                                                */
/* -------------------------------------------------------------------------- */

/// Create a mock input context and a handle to read its event log.
pub fn mock_context() -> (*mut TypioInputContext, ContextLogHandle) {
    let id = next_id();
    let ctx = Box::into_raw(Box::new(id)) as *mut TypioInputContext;
    (ctx, ContextLogHandle { id, ctx })
}

/// RAII handle to a mock context's event log; frees the context on drop.
pub struct ContextLogHandle {
    id: u64,
    ctx: *mut TypioInputContext,
}

impl ContextLogHandle {
    /// Drain all recorded events.
    pub fn take(&self) -> Vec<ContextEvent> {
        drain_log(self.id)
    }

    /// Clone current events without draining.
    pub fn clone_events(&self) -> Vec<ContextEvent> {
        peek_log(self.id)
    }
}

impl Drop for ContextLogHandle {
    fn drop(&mut self) {
        CONTEXT_LOGS.lock().unwrap().remove(&self.id);
        if !self.ctx.is_null() {
            unsafe { drop(Box::from_raw(self.ctx as *mut u64)) };
        }
    }
}

/// Create a mock `TypioInstance` exposing the given config values.
pub fn mock_instance(config: HashMap<String, ConfigValue>) -> *mut TypioInstance {
    let id = next_id();
    let inst = Box::into_raw(Box::new(id)) as *mut TypioInstance;
    CONFIGS.lock().unwrap().insert(id, config);
    inst
}

/// Free a mock instance and its config.
///
/// # Safety
/// `inst` must have come from [`mock_instance`] and not be freed twice.
pub unsafe fn free_instance(inst: *mut TypioInstance) {
    if inst.is_null() {
        return;
    }
    let id = *(inst as *mut u64);
    CONFIGS.lock().unwrap().remove(&id);
    drop(Box::from_raw(inst as *mut u64));
}

/* -------------------------------------------------------------------------- */
/* Event builders                                                             */
/* -------------------------------------------------------------------------- */

/// Build a key-press event for the given Unicode codepoint.
pub fn key_press(unicode: char) -> TypioKeyEvent {
    key_press_raw(unicode as u32, unicode as u32, 0)
}

/// Build a key-press event with full control over keysym/unicode/modifiers.
pub fn key_press_raw(keysym: u32, unicode: u32, modifiers: u32) -> TypioKeyEvent {
    TypioKeyEvent {
        struct_size: std::mem::size_of::<TypioKeyEvent>(),
        type_: TypioEventType::TypioEventKeyPress,
        keycode: 0,
        keysym,
        modifiers,
        unicode,
        time: 0,
        is_repeat: false,
        base_keysym: keysym,
    }
}

/// Build a modifier-only key event (e.g. Shift_L).
pub fn modifier_key(keysym: u32) -> TypioKeyEvent {
    key_press_raw(keysym, 0, TypioModifier::TypioModShift as u32)
}

/// Build an Escape key event.
pub fn escape_key() -> TypioKeyEvent {
    key_press_raw(TYPIO_KEY_Escape, 0x1B, 0)
}

/* -------------------------------------------------------------------------- */
/* TestHarness — ergonomic wrapper for Rust dev-dependency tests              */
/* -------------------------------------------------------------------------- */

/// Safe(ish) wrapper around a keyboard engine for integration tests.
///
/// # Safety
/// The engine pointer must remain valid for the lifetime of the harness.
pub struct TestHarness {
    pub engine: *mut TypioKeyboardEngine,
    pub ctx: *mut TypioInputContext,
    pub log: ContextLogHandle,
    pub instance: *mut TypioInstance,
}

impl TestHarness {
    /// Create and initialize a keyboard engine.
    ///
    /// # Safety
    /// `create` must be a valid factory exported by the engine under test.
    pub unsafe fn new_keyboard(
        create: unsafe extern "C" fn() -> *mut TypioKeyboardEngine,
        config: HashMap<String, ConfigValue>,
    ) -> Option<Self> {
        let engine = create();
        if engine.is_null() {
            return None;
        }
        let (ctx, log) = mock_context();
        let instance = mock_instance(config);

        let base = &mut (*engine).base;
        if let Some(init) = (*base.base_ops).init
            && init(base, instance) != TypioResult::TypioOk
        {
            free_instance(instance);
            return None;
        }

        Some(TestHarness {
            engine,
            ctx,
            log,
            instance,
        })
    }

    /// Send a key event to the engine.
    pub unsafe fn press(&mut self, event: &TypioKeyEvent) -> TypioKeyProcessResult {
        let kb = (*self.engine).keyboard;
        if kb.is_null() {
            return TypioKeyProcessResult::TypioKeyNotHandled;
        }
        match (*kb).process_key {
            Some(f) => f(self.engine, self.ctx, event as *const _ as *const c_void),
            None => TypioKeyProcessResult::TypioKeyNotHandled,
        }
    }

    pub unsafe fn focus_in(&mut self) {
        let base = &mut (*self.engine).base;
        if let Some(f) = (*base.base_ops).focus_in {
            f(base, self.ctx);
        }
    }

    pub unsafe fn focus_out(&mut self) {
        let base = &mut (*self.engine).base;
        if let Some(f) = (*base.base_ops).focus_out {
            f(base, self.ctx);
        }
    }

    pub unsafe fn reset(&mut self) {
        let base = &mut (*self.engine).base;
        if let Some(f) = (*base.base_ops).reset {
            f(base, self.ctx);
        }
    }

    pub unsafe fn reload_config(&mut self) -> TypioResult {
        let base = &mut (*self.engine).base;
        match (*base.base_ops).reload_config {
            Some(f) => f(base),
            None => TypioResult::TypioOk,
        }
    }

    pub unsafe fn deactivate(&mut self) {
        let base = &mut (*self.engine).base;
        if let Some(f) = (*base.base_ops).deactivate {
            f(base);
        }
    }

    /// Destroy the engine and clean up mocks.
    pub unsafe fn destroy(self) {
        let base = &mut (*self.engine).base;
        if let Some(destroy) = (*base.base_ops).destroy {
            destroy(base);
        }
        free_instance(self.instance);
        // ctx is freed when `log` (ContextLogHandle) drops.
        libc::free(self.engine as *mut c_void);
    }
}
