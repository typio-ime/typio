//! `typio-vet` — conformance vetting for native Typio C ABI engines.
//!
//! `vet` puts a single engine through three dimensions of the engine contract
//! and reports a `PASS`/`WARN`/`FAIL` verdict per check:
//!
//! * **ABI** — `TypioEngineInfo`, struct sizes, and vtable completeness.
//! * **Behavior** — invariants observed by driving the engine against a mock
//!   host (e.g. a committed key must emit a commit; a lone modifier must not).
//! * **Resource** — packaged assets that ship beside the native engine
//!   artifact, today the freedesktop icon contract.
//!
//! Only `FAIL` blocks the gate; `WARN` flags behavior that is legal but
//! suspicious, so a do-nothing engine cannot quietly pass clean.
//!
//! ## As a dev-dependency (Rust engine authors)
//!
//! ```rust,no_run
//! use typio_vet::{key_press, ContextEvent, TestHarness, TypioKeyProcessResult};
//!
//! # unsafe extern "C" fn my_keyboard_engine_create() -> *mut typio_vet::TypioKeyboardEngine { std::ptr::null_mut() }
//! #[test]
//! fn engine_commits_a() {
//!     let mut h = unsafe {
//!         TestHarness::new_keyboard(my_keyboard_engine_create, Default::default())
//!     }
//!     .expect("init failed");
//!     let r = unsafe { h.press(&key_press('a')) };
//!     assert_eq!(r, TypioKeyProcessResult::TypioKeyCommitted);
//!     assert_eq!(h.log.take(), vec![ContextEvent::Commit("a".into())]);
//!     unsafe { h.destroy() };
//! }
//! ```
//!
//! ## As a CLI
//!
//! ```bash
//! typio-vet ../typio-engine-basic/target/debug/libtypio_engine_basic.so
//! ```

use std::path::Path;

pub mod check;
pub mod mock;
pub mod resource;
pub mod scenario;

// The shared ABI types are part of this crate's public surface.
pub use typio_abi::*;

// Flat re-exports so the dev-dependency API is `typio_vet::Thing`.
pub use check::{CheckCategory, CheckResult, CheckStatus, Summary};
pub use mock::*;
pub use scenario::{KeyboardFactory, VoiceFactory};

/// Vet a keyboard engine across ABI, behavior, and (if `pkg` is known) resources.
///
/// # Safety
/// `create` must be the engine's real `typio_keyboard_engine_create` export and
/// `get_info` its `typio_engine_get_info`.
pub unsafe fn vet_keyboard(
    create: KeyboardFactory,
    get_info: unsafe extern "C" fn() -> *const TypioEngineInfo,
    pkg: Option<&Path>,
) -> Vec<CheckResult> {
    let mut out = scenario::keyboard_checks(create);
    out.extend(resource::resource_checks(get_info(), pkg));
    out
}

/// Vet a voice engine across ABI, behavior, and (if `pkg` is known) resources.
///
/// # Safety
/// See [`vet_keyboard`]; `create` must be `typio_voice_engine_create`.
pub unsafe fn vet_voice(
    create: VoiceFactory,
    get_info: unsafe extern "C" fn() -> *const TypioEngineInfo,
    pkg: Option<&Path>,
) -> Vec<CheckResult> {
    let mut out = scenario::voice_checks(create);
    out.extend(resource::resource_checks(get_info(), pkg));
    out
}
