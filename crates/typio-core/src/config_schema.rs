//! Config schema registry — Rust implementation of `typio/config_schema.h`.
//!
//! The schema is split into two layers:
//!
//! * A **static base** declared in this file, covering framework-owned
//!   settings (keyboard policy, notifications, shortcuts, voice runtime).
//!   No `engines.<name>.*` or `display.*` field lives here —
//!   every engine (including `basic`) ships as its own plugin and registers
//!   its own keys, and display/popup styling is owned by each frontend
//!   (`typio`, future GUI hosts) in its own config file.
//! * A **dynamic layer** populated at runtime via the registration API
//!   (`typio_config_schema_register*`). Engine plugins use this to declare
//!   their own `engines.<engine>.*` fields without core having to know about
//!   them, fulfilling the goal of ADR-style "engines own their config".
//!
//! Pointer-stability contract: pointers returned by `typio_config_schema_*`
//! remain valid until the next registration mutation (`register` /
//! `register_many` / `unregister`). Callers that need a longer-lived view must
//! copy the data out. In practice engines register their schema once at
//! plugin-load time before any UI consumer queries it.

use crate::config::Config;
use crate::types::*;
use std::ffi::{CStr, CString, c_char};
use std::os::raw::{c_double, c_int};
use std::ptr;
use std::sync::{LazyLock, RwLock};

/* -------------------------------------------------------------------------- */
/* Static base schema                                                         */
/* -------------------------------------------------------------------------- */
/// Wrapper to make raw-pointer Vecs usable in a LazyLock static.
struct SyncCStrVec(Vec<*const c_char>);
unsafe impl Sync for SyncCStrVec {}
unsafe impl Send for SyncCStrVec {}

/// Schema entry definition — Rust-native form for the static base.
struct SchemaEntry {
    key: &'static str,
    type_: TypioFieldType,
    def: SchemaDefault,
    ui_label: Option<&'static str>,
    ui_section: Option<&'static str>,
    ui_min: c_int,
    ui_max: c_int,
    ui_step: c_int,
    ui_options: Option<&'static SyncCStrVec>,
    runtime_property: Option<&'static str>,
}

#[derive(Clone)]
enum SchemaDefault {
    String(&'static str),
    Int(c_int),
    Bool(bool),
    #[allow(dead_code)]
    Float(c_double),
}

static SCHEMA: LazyLock<Vec<SchemaEntry>> = LazyLock::new(|| {
    vec![
        SchemaEntry {
            key: "keyboard.per_app_preferences",
            type_: TypioFieldType::TypioFieldBool,
            def: SchemaDefault::Bool(true),
            ui_label: Some("Per-app preferences"),
            ui_section: Some("keyboard"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "notifications.enable",
            type_: TypioFieldType::TypioFieldBool,
            def: SchemaDefault::Bool(true),
            ui_label: Some("Enable"),
            ui_section: Some("notifications"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "notifications.startup_checks",
            type_: TypioFieldType::TypioFieldBool,
            def: SchemaDefault::Bool(true),
            ui_label: Some("Startup checks"),
            ui_section: Some("notifications"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "notifications.runtime",
            type_: TypioFieldType::TypioFieldBool,
            def: SchemaDefault::Bool(true),
            ui_label: Some("Runtime alerts"),
            ui_section: Some("notifications"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "notifications.voice",
            type_: TypioFieldType::TypioFieldBool,
            def: SchemaDefault::Bool(true),
            ui_label: Some("Voice alerts"),
            ui_section: Some("notifications"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "notifications.cooldown_ms",
            type_: TypioFieldType::TypioFieldInt,
            def: SchemaDefault::Int(15000),
            ui_label: Some("Cooldown (ms)"),
            ui_section: Some("notifications"),
            ui_min: 0,
            ui_max: 300000,
            ui_step: 1000,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "shortcuts.switch_language",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String("Ctrl+Shift"),
            ui_label: Some("Switch language"),
            ui_section: Some("shortcuts"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "shortcuts.switch_keyboard_engine",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String(""),
            ui_label: Some("Switch keyboard engine"),
            ui_section: Some("shortcuts"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "languages.enabled",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String(""),
            ui_label: Some("Enabled languages"),
            ui_section: Some("languages"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "shortcuts.exit",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String("Ctrl+Shift+Escape"),
            ui_label: Some("Exit"),
            ui_section: Some("shortcuts"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "shortcuts.voice_ptt",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String("Super+v"),
            ui_label: Some("Voice (PTT)"),
            ui_section: Some("shortcuts"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "keyboard.engine",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String(""),
            ui_label: Some("Keyboard engine"),
            ui_section: Some("keyboard"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "keyboard.disabled",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String(""),
            ui_label: Some("Disabled keyboard engines"),
            ui_section: Some("keyboard"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "voice.engine",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String(""),
            ui_label: Some("Voice engine"),
            ui_section: Some("voice"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
        SchemaEntry {
            key: "voice.disabled",
            type_: TypioFieldType::TypioFieldString,
            def: SchemaDefault::String(""),
            ui_label: Some("Disabled voice engines"),
            ui_section: Some("voice"),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: None,
            runtime_property: None,
        },
    ]
});

/* -------------------------------------------------------------------------- */
/* Dynamic schema entry — owned copy of caller-supplied data                  */
/* -------------------------------------------------------------------------- */

/// Owned storage for one dynamically-registered field. Stored in a `Box`
/// inside the registry so its address (and therefore every `*const c_char`
/// derived from it) stays stable while it is registered.
struct DynamicEntry {
    key: CString,
    type_: TypioFieldType,
    def_string: Option<CString>,
    def_int: c_int,
    def_bool: bool,
    def_float: c_double,
    ui_label: Option<CString>,
    ui_section: Option<CString>,
    ui_min: c_int,
    ui_max: c_int,
    ui_step: c_int,
    // Storage for ui_options strings plus the NULL-terminated pointer array
    // that mirrors them.
    ui_options_storage: Vec<CString>,
    ui_options_ptrs: Vec<*const c_char>,
    runtime_property: Option<CString>,
}

// SAFETY: DynamicEntry owns its storage; the raw pointers it exposes are
// derived from heap allocations owned by this struct. The registry is
// guarded by an RwLock for cross-thread access.
unsafe impl Send for DynamicEntry {}
unsafe impl Sync for DynamicEntry {}

impl DynamicEntry {
    fn from_field(src: &TypioConfigField) -> Result<Box<Self>, TypioResult> {
        if src.key.is_null() {
            return Err(TypioResult::TypioErrorInvalidArgument);
        }
        let key = unsafe { CStr::from_ptr(src.key) }.to_owned();
        if key.as_bytes().is_empty() {
            return Err(TypioResult::TypioErrorInvalidArgument);
        }

        let mut def_string = None;
        let mut def_int = 0;
        let mut def_bool = false;
        let mut def_float = 0.0;
        match src.type_ {
            TypioFieldType::TypioFieldString => {
                let s = unsafe { src.def.s };
                if !s.is_null() {
                    def_string = Some(unsafe { CStr::from_ptr(s) }.to_owned());
                }
            }
            TypioFieldType::TypioFieldInt => def_int = unsafe { src.def.i },
            TypioFieldType::TypioFieldBool => def_bool = unsafe { src.def.b },
            TypioFieldType::TypioFieldFloat => def_float = unsafe { src.def.f },
        }

        let ui_label = clone_opt_cstring(src.ui_label);
        let ui_section = clone_opt_cstring(src.ui_section);
        let runtime_property = clone_opt_cstring(src.runtime_property);

        let ui_options_storage = clone_options(src.ui_options);

        let mut boxed = Box::new(Self {
            key,
            type_: src.type_,
            def_string,
            def_int,
            def_bool,
            def_float,
            ui_label,
            ui_section,
            ui_min: src.ui_min,
            ui_max: src.ui_max,
            ui_step: src.ui_step,
            ui_options_storage,
            ui_options_ptrs: Vec::new(),
            runtime_property,
        });
        // Pointers into each owned CString remain stable across `Vec` moves
        // because CString stores its bytes in a heap `Box<[u8]>`. Build the
        // NULL-terminated `*const c_char` array once and shrink to its final
        // capacity to lock the data pointer.
        boxed.ui_options_ptrs = boxed
            .ui_options_storage
            .iter()
            .map(|s| s.as_ptr())
            .chain(std::iter::once(ptr::null()))
            .collect();
        boxed.ui_options_ptrs.shrink_to_fit();
        Ok(boxed)
    }

    fn build_view(&self) -> TypioConfigField {
        let def = match self.type_ {
            TypioFieldType::TypioFieldString => TypioFieldDefault {
                s: self.def_string.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
            },
            TypioFieldType::TypioFieldInt => TypioFieldDefault { i: self.def_int },
            TypioFieldType::TypioFieldBool => TypioFieldDefault { b: self.def_bool },
            TypioFieldType::TypioFieldFloat => TypioFieldDefault { f: self.def_float },
        };
        TypioConfigField {
            key: self.key.as_ptr(),
            type_: self.type_,
            def,
            ui_label: opt_cstring_ptr(&self.ui_label),
            ui_section: opt_cstring_ptr(&self.ui_section),
            ui_min: self.ui_min,
            ui_max: self.ui_max,
            ui_step: self.ui_step,
            ui_options: if self.ui_options_storage.is_empty() {
                ptr::null()
            } else {
                self.ui_options_ptrs.as_ptr()
            },
            runtime_property: opt_cstring_ptr(&self.runtime_property),
        }
    }
}

fn clone_opt_cstring(p: *const c_char) -> Option<CString> {
    if p.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(p) }.to_owned())
}

fn opt_cstring_ptr(opt: &Option<CString>) -> *const c_char {
    match opt {
        Some(c) => c.as_ptr(),
        None => ptr::null(),
    }
}

/// Deep-copy a NULL-terminated `const char *const *` array into owned storage.
fn clone_options(opts: *const *const c_char) -> Vec<CString> {
    if opts.is_null() {
        return Vec::new();
    }
    let mut storage = Vec::new();
    let mut cursor = opts;
    unsafe {
        while !(*cursor).is_null() {
            storage.push(CStr::from_ptr(*cursor).to_owned());
            cursor = cursor.add(1);
        }
    }
    storage
}

/* -------------------------------------------------------------------------- */
/* Combined schema registry (static base + dynamic)                           */
/* -------------------------------------------------------------------------- */

struct Registry {
    dynamic: Vec<Box<DynamicEntry>>,
    /// Cached C-compatible flat view: static entries first, dynamic after.
    /// Rebuilt on every register/unregister.
    combined: Vec<TypioConfigField>,
}

// SAFETY: Registry's combined Vec contains raw pointers, but they all point
// either into leaked static CStrings (static entries) or into Box<DynamicEntry>
// heap allocations owned by `dynamic`. The RwLock guards concurrent access.
unsafe impl Send for Registry {}
unsafe impl Sync for Registry {}

impl Registry {
    fn new() -> Self {
        let mut r = Self {
            dynamic: Vec::new(),
            combined: Vec::new(),
        };
        r.rebuild();
        r
    }

    fn rebuild(&mut self) {
        let mut combined = Vec::with_capacity(STATIC_VIEW.0.len() + self.dynamic.len());
        combined.extend(STATIC_VIEW.0.iter().map(|f| TypioConfigField {
            key: f.key,
            type_: f.type_,
            def: unsafe { f.def.raw_copy() },
            ui_label: f.ui_label,
            ui_section: f.ui_section,
            ui_min: f.ui_min,
            ui_max: f.ui_max,
            ui_step: f.ui_step,
            ui_options: f.ui_options,
            runtime_property: f.runtime_property,
        }));
        for e in self.dynamic.iter() {
            combined.push(e.build_view());
        }
        self.combined = combined;
    }

    fn find_dynamic(&self, key: &str) -> Option<usize> {
        self.dynamic
            .iter()
            .position(|e| e.key.to_str().map(|s| s == key).unwrap_or(false))
    }

    fn key_in_static(key: &str) -> bool {
        SCHEMA.iter().any(|e| e.key == key)
    }

    fn register(&mut self, field: &TypioConfigField) -> TypioResult {
        let entry = match DynamicEntry::from_field(field) {
            Ok(e) => e,
            Err(r) => return r,
        };
        let key_str = match entry.key.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return TypioResult::TypioErrorInvalidArgument,
        };
        if Self::key_in_static(&key_str) || self.find_dynamic(&key_str).is_some() {
            return TypioResult::TypioErrorAlreadyExists;
        }
        self.dynamic.push(entry);
        self.rebuild();
        TypioResult::TypioOk
    }

    fn unregister(&mut self, key: &str) -> TypioResult {
        match self.find_dynamic(key) {
            Some(idx) => {
                self.dynamic.remove(idx);
                self.rebuild();
                TypioResult::TypioOk
            }
            None => TypioResult::TypioErrorNotFound,
        }
    }
}

/// Static-only view, built once and reused as the base of every rebuild.
struct StaticView(Vec<TypioConfigField>);
unsafe impl Sync for StaticView {}
unsafe impl Send for StaticView {}

static STATIC_VIEW: LazyLock<StaticView> = LazyLock::new(|| {
    StaticView(
        SCHEMA
            .iter()
            .map(|e| {
                let def = match e.def {
                    SchemaDefault::String(s) => TypioFieldDefault {
                        s: CString::new(s).unwrap().into_raw(),
                    },
                    SchemaDefault::Int(i) => TypioFieldDefault { i },
                    SchemaDefault::Bool(b) => TypioFieldDefault { b },
                    SchemaDefault::Float(f) => TypioFieldDefault { f },
                };
                TypioConfigField {
                    key: CString::new(e.key).unwrap().into_raw(),
                    type_: e.type_,
                    def,
                    ui_label: e.ui_label.map_or(ptr::null(), |s| {
                        CString::new(s).unwrap().into_raw() as *const c_char
                    }),
                    ui_section: e.ui_section.map_or(ptr::null(), |s| {
                        CString::new(s).unwrap().into_raw() as *const c_char
                    }),
                    ui_min: e.ui_min,
                    ui_max: e.ui_max,
                    ui_step: e.ui_step,
                    ui_options: e.ui_options.map_or(ptr::null(), |opts| opts.0.as_ptr()),
                    runtime_property: e.runtime_property.map_or(ptr::null(), |s| {
                        CString::new(s).unwrap().into_raw() as *const c_char
                    }),
                }
            })
            .collect(),
    )
});

static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(|| RwLock::new(Registry::new()));

/* -------------------------------------------------------------------------- */
/* C FFI — read                                                               */
/* -------------------------------------------------------------------------- */

/// Return the full combined schema (static base + dynamic entries).
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_fields(count: *mut usize) -> *const TypioConfigField {
    let guard = REGISTRY.read().expect("schema registry poisoned");
    if !count.is_null() {
        unsafe { *count = guard.combined.len() };
    }
    guard.combined.as_ptr()
}

/// Look up a single schema field by key.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_find(key: *const c_char) -> *const TypioConfigField {
    if key.is_null() {
        return ptr::null();
    }
    let key_str = unsafe { CStr::from_ptr(key).to_string_lossy() };
    let guard = REGISTRY.read().expect("schema registry poisoned");
    guard
        .combined
        .iter()
        .find(|f| {
            let f_key = unsafe { CStr::from_ptr(f.key).to_string_lossy() };
            f_key == key_str.as_ref()
        })
        .map_or(ptr::null(), |f| f as *const TypioConfigField)
}

/// Return pointers to every schema field whose `key` starts with `prefix`
/// (ADR-0008).
///
/// Borrowed: the returned array and the `TypioConfigField *` it contains are
/// owned by the registry and remain valid until the next
/// `typio_config_schema_register*`/`_unregister` call. Caller frees the
/// returned outer array (not the pointed-to fields) with
/// `typio_config_schema_fields_with_prefix_free`.
///
/// Returns NULL with `*out_count = 0` when `prefix` is NULL or no field
/// matches.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_fields_with_prefix(
    prefix: *const c_char,
    out_count: *mut usize,
) -> *mut *const TypioConfigField {
    if !out_count.is_null() {
        unsafe { *out_count = 0 };
    }
    if prefix.is_null() || out_count.is_null() {
        return ptr::null_mut();
    }
    let prefix_str = unsafe { CStr::from_ptr(prefix).to_string_lossy() };
    let guard = REGISTRY.read().expect("schema registry poisoned");
    let mut matches: Vec<*const TypioConfigField> = guard
        .combined
        .iter()
        .filter(|f| {
            let f_key = unsafe { CStr::from_ptr(f.key).to_string_lossy() };
            f_key.starts_with(prefix_str.as_ref())
        })
        .map(|f| f as *const TypioConfigField)
        .collect();
    if matches.is_empty() {
        return ptr::null_mut();
    }
    unsafe { *out_count = matches.len() };
    let ptr = matches.as_mut_ptr();
    std::mem::forget(matches);
    ptr
}

/// Release the outer array returned by `typio_config_schema_fields_with_prefix`.
///
/// Does NOT free the pointed-to `TypioConfigField`s; those are owned by the
/// schema registry.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_fields_with_prefix_free(
    fields: *mut *const TypioConfigField,
    count: usize,
) {
    if fields.is_null() || count == 0 {
        return;
    }
    unsafe {
        let _ = Vec::from_raw_parts(fields, count, count);
    }
}

/// Return the runtime property string for a schema key, if any.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_runtime_property(key: *const c_char) -> *const c_char {
    let field = typio_config_schema_find(key);
    if field.is_null() {
        return ptr::null();
    }
    let f = unsafe { &*field };
    if f.runtime_property.is_null() {
        return ptr::null();
    }
    let prop = unsafe { CStr::from_ptr(f.runtime_property).to_string_lossy() };
    if prop.is_empty() {
        return ptr::null();
    }
    f.runtime_property
}

/// Populate missing config keys with their schema default values.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_apply_defaults(config: *mut Config) {
    if config.is_null() {
        return;
    }
    let cfg = unsafe { &mut *config };

    // Static defaults (preserves the Rust-native typed `SchemaDefault` so
    // string defaults distinguish "empty -> skip" from "set to ''").
    for entry in SCHEMA.iter() {
        let key = entry.key;
        if cfg.entries.contains_key(key) {
            continue;
        }
        match &entry.def {
            SchemaDefault::String(s) if !s.is_empty() => {
                cfg.set_value(
                    key.to_string(),
                    crate::config::ConfigValue::String(CString::new(s.to_string()).unwrap()),
                );
            }
            SchemaDefault::Int(i) => {
                cfg.set_value(key.to_string(), crate::config::ConfigValue::Int(*i));
            }
            SchemaDefault::Bool(b) => {
                cfg.set_value(key.to_string(), crate::config::ConfigValue::Bool(*b));
            }
            SchemaDefault::Float(f) => {
                cfg.set_value(key.to_string(), crate::config::ConfigValue::Float(*f));
            }
            _ => {}
        }
    }

    // Dynamic (engine-registered) defaults.
    let guard = REGISTRY.read().expect("schema registry poisoned");
    for e in guard.dynamic.iter() {
        let key = match e.key.to_str() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if cfg.entries.contains_key(key) {
            continue;
        }
        match e.type_ {
            TypioFieldType::TypioFieldString => {
                if let Some(s) = e.def_string.as_ref() {
                    let bytes = s.as_bytes();
                    if !bytes.is_empty() {
                        cfg.set_value(
                            key.to_string(),
                            crate::config::ConfigValue::String(s.clone()),
                        );
                    }
                }
            }
            TypioFieldType::TypioFieldInt => {
                cfg.set_value(key.to_string(), crate::config::ConfigValue::Int(e.def_int));
            }
            TypioFieldType::TypioFieldBool => {
                cfg.set_value(
                    key.to_string(),
                    crate::config::ConfigValue::Bool(e.def_bool),
                );
            }
            TypioFieldType::TypioFieldFloat => {
                cfg.set_value(
                    key.to_string(),
                    crate::config::ConfigValue::Float(e.def_float),
                );
            }
        }
    }
}

/* -------------------------------------------------------------------------- */
/* C FFI — register / unregister                                              */
/* -------------------------------------------------------------------------- */

/// Register a single engine-owned schema field.
///
/// All strings reachable from `field` are deep-copied; the caller may free
/// (or stack-drop) its memory after the call returns. Returns
/// `TypioErrorAlreadyExists` if `field.key` collides with an existing static
/// or dynamic key.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_register(field: *const TypioConfigField) -> TypioResult {
    if field.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let f = unsafe { &*field };
    let mut guard = REGISTRY.write().expect("schema registry poisoned");
    guard.register(f)
}

/// Register an array of schema fields. Stops at and returns the first error
/// encountered; fields registered before the error remain registered.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_register_many(
    fields: *const TypioConfigField,
    count: usize,
) -> TypioResult {
    if fields.is_null() && count > 0 {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let mut guard = REGISTRY.write().expect("schema registry poisoned");
    for i in 0..count {
        let f = unsafe { &*fields.add(i) };
        let r = guard.register(f);
        if r != TypioResult::TypioOk {
            return r;
        }
    }
    TypioResult::TypioOk
}

/// Remove a previously-registered dynamic field by key. Static fields cannot
/// be unregistered and produce `TypioErrorNotFound`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_config_schema_unregister(key: *const c_char) -> TypioResult {
    if key.is_null() {
        return TypioResult::TypioErrorInvalidArgument;
    }
    let key_str = match unsafe { CStr::from_ptr(key) }.to_str() {
        Ok(s) => s,
        Err(_) => return TypioResult::TypioErrorInvalidArgument,
    };
    let mut guard = REGISTRY.write().expect("schema registry poisoned");
    guard.unregister(key_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use std::ffi::{CStr, CString};
    use std::ptr;
    use std::sync::Mutex;

    // Registration mutates global state; serialize tests that touch it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn apply_defaults_sets_known_keys() {
        let _g = TEST_LOCK.lock().unwrap();
        let cfg = config::typio_config_new();
        assert!(!cfg.is_null());
        typio_config_apply_defaults(cfg);

        let key = CString::new("notifications.cooldown_ms").unwrap();
        let got = config::typio_config_get_int(cfg, key.as_ptr(), 0);
        assert_eq!(got, 15000);

        let key2 = CString::new("shortcuts.switch_language").unwrap();
        let got2 = config::typio_config_get_string(cfg, key2.as_ptr(), ptr::null());
        assert!(!got2.is_null());
        let s = unsafe { CStr::from_ptr(got2) }.to_str().unwrap();
        assert_eq!(s, "Ctrl+Shift");

        // Empty-string defaults are "skip", so the engine-cycling chord is
        // absent from a default config (the action stays unbound).
        let key2b = CString::new("shortcuts.switch_keyboard_engine").unwrap();
        let got2b = config::typio_config_get_string(cfg, key2b.as_ptr(), ptr::null());
        assert!(got2b.is_null());

        let key3 = CString::new("keyboard.per_app_preferences").unwrap();
        let got3 = config::typio_config_get_bool(cfg, key3.as_ptr(), false);
        assert!(got3);

        config::typio_config_free(cfg);
    }

    #[test]
    fn display_fields_are_not_built_in() {
        let _g = TEST_LOCK.lock().unwrap();
        for k in [
            "display.panel_theme",
            "display.candidate_layout",
            "display.font_size",
            "display.font_family",
            "display.popup_mode_indicator",
        ] {
            let key = CString::new(k).unwrap();
            assert!(
                typio_config_schema_find(key.as_ptr()).is_null(),
                "{} should not be statically registered — owned by frontends",
                k
            );
        }
    }

    #[test]
    fn schema_fields_non_empty() {
        let _g = TEST_LOCK.lock().unwrap();
        let mut count: usize = 0;
        let fields = typio_config_schema_fields(&mut count);
        assert!(!fields.is_null());
        assert!(count > 0);
    }

    #[test]
    fn schema_find_existing() {
        let _g = TEST_LOCK.lock().unwrap();
        let key = CString::new("shortcuts.switch_keyboard_engine").unwrap();
        let field = typio_config_schema_find(key.as_ptr());
        assert!(!field.is_null());
    }

    #[test]
    fn schema_find_missing() {
        let _g = TEST_LOCK.lock().unwrap();
        let key = CString::new("nonexistent.key").unwrap();
        let field = typio_config_schema_find(key.as_ptr());
        assert!(field.is_null());
    }

    #[test]
    fn engine_fields_are_not_built_in() {
        let _g = TEST_LOCK.lock().unwrap();
        for k in [
            "engines.rime.shared_data_dir",
            "engines.rime.user_data_dir",
            "engines.compose.printable_key_mode",
            "engines.compose.compose",
        ] {
            let key = CString::new(k).unwrap();
            assert!(
                typio_config_schema_find(key.as_ptr()).is_null(),
                "{} should not be statically registered",
                k
            );
        }
    }

    #[test]
    fn register_and_lookup_dynamic_field() {
        let _g = TEST_LOCK.lock().unwrap();
        let key = CString::new("engines.test_dyn.opt_int").unwrap();
        let label = CString::new("Test option").unwrap();
        let section = CString::new("test_dyn").unwrap();
        let field = TypioConfigField {
            key: key.as_ptr(),
            type_: TypioFieldType::TypioFieldInt,
            def: TypioFieldDefault { i: 42 },
            ui_label: label.as_ptr(),
            ui_section: section.as_ptr(),
            ui_min: 0,
            ui_max: 100,
            ui_step: 1,
            ui_options: ptr::null(),
            runtime_property: ptr::null(),
        };
        assert_eq!(typio_config_schema_register(&field), TypioResult::TypioOk);

        let found = typio_config_schema_find(key.as_ptr());
        assert!(!found.is_null());
        let f = unsafe { &*found };
        assert_eq!(f.type_, TypioFieldType::TypioFieldInt);
        assert_eq!(unsafe { f.def.i }, 42);

        // apply_defaults should populate the dynamic key.
        let cfg = config::typio_config_new();
        typio_config_apply_defaults(cfg);
        assert_eq!(config::typio_config_get_int(cfg, key.as_ptr(), -1), 42);
        config::typio_config_free(cfg);

        assert_eq!(
            typio_config_schema_unregister(key.as_ptr()),
            TypioResult::TypioOk
        );
        assert!(typio_config_schema_find(key.as_ptr()).is_null());
    }

    #[test]
    fn register_string_with_options() {
        let _g = TEST_LOCK.lock().unwrap();
        let key = CString::new("engines.test_dyn.mode").unwrap();
        let default = CString::new("alpha").unwrap();
        let opt_a = CString::new("alpha").unwrap();
        let opt_b = CString::new("beta").unwrap();
        let options: [*const c_char; 3] = [opt_a.as_ptr(), opt_b.as_ptr(), ptr::null()];
        let field = TypioConfigField {
            key: key.as_ptr(),
            type_: TypioFieldType::TypioFieldString,
            def: TypioFieldDefault {
                s: default.as_ptr(),
            },
            ui_label: ptr::null(),
            ui_section: ptr::null(),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: options.as_ptr(),
            runtime_property: ptr::null(),
        };
        assert_eq!(typio_config_schema_register(&field), TypioResult::TypioOk);

        let found = typio_config_schema_find(key.as_ptr());
        assert!(!found.is_null());
        let f = unsafe { &*found };
        assert!(!f.ui_options.is_null());
        let first = unsafe { CStr::from_ptr(*f.ui_options) }.to_str().unwrap();
        assert_eq!(first, "alpha");
        let second = unsafe { CStr::from_ptr(*f.ui_options.add(1)) }
            .to_str()
            .unwrap();
        assert_eq!(second, "beta");
        assert!(unsafe { (*f.ui_options.add(2)).is_null() });

        assert_eq!(
            typio_config_schema_unregister(key.as_ptr()),
            TypioResult::TypioOk
        );
    }

    #[test]
    fn duplicate_registration_rejected() {
        let _g = TEST_LOCK.lock().unwrap();
        let key = CString::new("engines.test_dyn.dup").unwrap();
        let field = TypioConfigField {
            key: key.as_ptr(),
            type_: TypioFieldType::TypioFieldBool,
            def: TypioFieldDefault { b: true },
            ui_label: ptr::null(),
            ui_section: ptr::null(),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: ptr::null(),
            runtime_property: ptr::null(),
        };
        assert_eq!(typio_config_schema_register(&field), TypioResult::TypioOk);
        assert_eq!(
            typio_config_schema_register(&field),
            TypioResult::TypioErrorAlreadyExists
        );
        // Also rejected for static-key collision.
        let static_key = CString::new("notifications.cooldown_ms").unwrap();
        let collide = TypioConfigField {
            key: static_key.as_ptr(),
            type_: TypioFieldType::TypioFieldInt,
            def: TypioFieldDefault { i: 99 },
            ui_label: ptr::null(),
            ui_section: ptr::null(),
            ui_min: 0,
            ui_max: 0,
            ui_step: 0,
            ui_options: ptr::null(),
            runtime_property: ptr::null(),
        };
        assert_eq!(
            typio_config_schema_register(&collide),
            TypioResult::TypioErrorAlreadyExists
        );
        typio_config_schema_unregister(key.as_ptr());
    }
}
