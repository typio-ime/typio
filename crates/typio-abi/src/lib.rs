//! Typio C ABI type definitions.
//!
//! This crate contains the shared C-compatible types that form the contract
//! between the Typio host (libtypio), engine plugins, and test/lint tools.
//!
//! Opaque types (`TypioInputContext`, `TypioInstance`, `TypioRegistry`,
//! `TypioVoiceSession`) are declared with zero-size so they can be used as
//! pointers across the FFI boundary. The actual layout is private to the host.

#![allow(non_upper_case_globals)]

use std::ffi::{c_char, c_void};
use std::os::raw::{c_double, c_int};

/* -------------------------------------------------------------------------- */
/* Opaque handles                                                             */
/* -------------------------------------------------------------------------- */

/// Opaque input context handle — engines never dereference this.
#[repr(C)]
pub struct TypioInputContext {
    _opaque: [u8; 0],
}

/// Opaque instance handle — engines never dereference this.
#[repr(C)]
pub struct TypioInstance {
    _opaque: [u8; 0],
}

/// Opaque registry handle — engines never dereference this.
#[repr(C)]
pub struct TypioRegistry {
    _opaque: [u8; 0],
}

/// Opaque voice session handle — defined in C.
#[repr(C)]
pub struct TypioVoiceSession {
    _opaque: [u8; 0],
}

/* -------------------------------------------------------------------------- */
/* Result codes                                                               */
/* -------------------------------------------------------------------------- */

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioResult {
    TypioOk = 0,
    TypioError = -1,
    TypioErrorInvalidArgument = -2,
    TypioErrorOutOfMemory = -3,
    TypioErrorNotFound = -4,
    TypioErrorAlreadyExists = -5,
    TypioErrorNotInitialized = -6,
    TypioErrorEngineLoadFailed = -7,
    TypioErrorEngineNotAvailable = -8,
}

/* -------------------------------------------------------------------------- */
/* Config types                                                               */
/* -------------------------------------------------------------------------- */

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioConfigType {
    TypioConfigString = 0,
    TypioConfigInt = 1,
    TypioConfigBool = 2,
    TypioConfigFloat = 3,
    TypioConfigArray = 4,
    TypioConfigObject = 5,
}

/* -------------------------------------------------------------------------- */
/* Schema types                                                               */
/* -------------------------------------------------------------------------- */

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioFieldType {
    TypioFieldString = 0,
    TypioFieldInt = 1,
    TypioFieldBool = 2,
    TypioFieldFloat = 3,
}

/// Default value for a config schema field.
///
/// # Safety
/// This union contains a raw pointer in the `s` variant. It does **not**
/// implement `Copy` or `Clone` because duplicating the pointer value without
/// duplicating the pointed-to data is a use-after-free risk. C code may still
/// assign or memcopy this union (that is C's semantics), but Rust code must
/// treat it as a move-only type and never assume a copy is deep.
#[repr(C)]
pub union TypioFieldDefault {
    pub s: *const c_char,
    pub i: c_int,
    pub b: bool,
    pub f: c_double,
}

impl TypioFieldDefault {
    /// Explicit bitwise copy.
    ///
    /// # Safety
    /// Safe because every variant contains only `Copy` types (raw pointers,
    /// integers, bool, double). The caller must still manage the lifetime of
    /// any pointed-to data; this function does not allocate or free.
    pub unsafe fn raw_copy(&self) -> Self {
        std::ptr::read(self)
    }
}

#[repr(C)]
pub struct TypioConfigField {
    pub key: *const c_char,
    pub type_: TypioFieldType,
    pub def: TypioFieldDefault,
    pub ui_label: *const c_char,
    pub ui_section: *const c_char,
    pub ui_min: c_int,
    pub ui_max: c_int,
    pub ui_step: c_int,
    pub ui_options: *const *const c_char,
    pub runtime_property: *const c_char,
}

/* -------------------------------------------------------------------------- */
/* Engine types                                                               */
/* -------------------------------------------------------------------------- */

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioEngineType {
    TypioEngineTypeKeyboard = 0,
    TypioEngineTypeVoice = 1,
    TypioEngineTypeHandwriting = 2,
    TypioEngineTypeCustom = 100,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioStatusSalience {
    TypioStatusSalienceQuiet = 0,
    TypioStatusSalienceNotable = 1,
}

#[repr(C)]
pub struct TypioKeyboardEngineMode {
    pub id: *const c_char,
    pub label: *const c_char,
    pub display_label: *const c_char,
    pub icon_name: *const c_char,
    pub profile_id: *const c_char,
    pub profile_label: *const c_char,
    pub description: *const c_char,
    pub salience: TypioStatusSalience,
}

unsafe impl Sync for TypioKeyboardEngineMode {}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioKeyProcessResult {
    TypioKeyNotHandled = 0,
    TypioKeyHandled = 1,
    TypioKeyComposing = 2,
    TypioKeyCommitted = 3,
}

/// Engine availability lifecycle axis (ADR-0014). NULL availability op means
/// always `Ready`. The host must not route input unless `Ready`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioEngineAvailability {
    TypioEngineUninitialized = 0,
    TypioEnginePreparing = 1,
    TypioEngineReady = 2,
    TypioEngineFailed = 3,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioEventType {
    TypioEventKeyPress = 0,
    TypioEventKeyRelease = 1,
    TypioEventFocusIn = 2,
    TypioEventFocusOut = 3,
    TypioEventReset = 4,
    TypioEventVoiceStart = 5,
    TypioEventVoiceEnd = 6,
    TypioEventVoiceData = 7,
    TypioEventCommit = 8,
    TypioEventCandidateSelect = 9,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioModifier {
    TypioModNone = 0,
    TypioModShift = 1 << 0,
    TypioModCtrl = 1 << 1,
    TypioModAlt = 1 << 2,
    TypioModSuper = 1 << 3,
    TypioModCapslock = 1 << 4,
    TypioModNumlock = 1 << 5,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TypioKeyEvent {
    /// `sizeof(TypioKeyEvent)` at the caller's build time.
    pub struct_size: usize,
    pub type_: TypioEventType,
    pub keycode: u32,
    pub keysym: u32,
    pub modifiers: u32,
    pub unicode: u32,
    pub time: u64,
    pub is_repeat: bool,
    pub base_keysym: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TypioVoiceEvent {
    pub type_: TypioEventType,
    pub audio_data: *const c_void,
    pub audio_size: usize,
    pub sample_rate: c_int,
    pub channels: c_int,
    pub bits_per_sample: c_int,
}

#[repr(C)]
pub struct TypioEvent {
    pub type_: TypioEventType,
    pub time: u64,
    pub data: TypioEventData,
}

impl Clone for TypioEvent {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for TypioEvent {}

#[repr(C)]
#[derive(Copy)]
pub union TypioEventData {
    pub key: TypioKeyEvent,
    pub voice: TypioVoiceEvent,
}

impl Clone for TypioEventData {
    fn clone(&self) -> Self {
        *self
    }
}

/* -------------------------------------------------------------------------- */
/* Preedit / Candidate / Composition types                                    */
/* -------------------------------------------------------------------------- */

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioPreeditFormat {
    TypioPreeditNone = 0,
    TypioPreeditUnderline = 1 << 0,
    TypioPreeditHighlight = 1 << 1,
    TypioPreeditBold = 1 << 2,
    TypioPreeditItalic = 1 << 3,
}

#[repr(C)]
pub struct TypioPreeditSegment {
    pub text: *const c_char,
    pub format: u32,
}

#[repr(C)]
pub struct TypioPreedit {
    pub segments: *mut TypioPreeditSegment,
    pub segment_count: usize,
    pub cursor_pos: i32,
}

#[repr(C)]
pub struct TypioCandidate {
    pub text: *const c_char,
    pub comment: *const c_char,
    pub label: *const c_char,
}

/// Host-managed candidate selection flags (ADR-0013).
/// Engine declares which selection operations the host should intercept.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioHostManagedSelection {
    /// No host-managed selection; engine handles everything.
    TypioHostSelNone = 0,
    /// Host intercepts Up/Down/Left/Right for candidate navigation.
    TypioHostSelNavigate = 1 << 0,
    /// Host intercepts Space to commit the selected candidate.
    TypioHostSelCommit = 1 << 1,
    /// Host intercepts digit keys 0–9 to select candidate by index (1–9 → index 0–8, 0 → index 9).
    TypioHostSelIndexPick = 1 << 2,
    /// Host intercepts Enter/KP_Enter to commit the raw preedit buffer text.
    TypioHostSelCommitRaw = 1 << 3,
    TypioHostSelAll = 0xF,
}

/// Atomic composition snapshot: preedit + candidates as one value.
/// All pointers are borrowed and valid only for the call/callback duration.
#[repr(C)]
pub struct TypioComposition {
    pub struct_size: usize,
    /* preedit */
    pub segments: *const TypioPreeditSegment,
    pub segment_count: usize,
    pub cursor_pos: i32,
    /* candidates */
    pub candidates: *const TypioCandidate,
    pub candidate_count: usize,
    pub page: i32,
    pub page_size: i32,
    pub total: i32,
    pub selected: i32,
    pub has_prev: bool,
    pub has_next: bool,
    pub content_signature: u64,
    pub revision: u64,
    pub host_managed_selection: u32,
}

/* -------------------------------------------------------------------------- */
/* Engine info / vtable / surface                                             */
/* -------------------------------------------------------------------------- */

/// Current engine ABI major version (incompatible changes).
pub const TYPIO_ENGINE_ABI_MAJOR: u32 = 0;
/// Current engine ABI minor version (backward-compatible additions).
pub const TYPIO_ENGINE_ABI_MINOR: u32 = 2;

/// ABI version an engine reports through `typio_engine_abi_version`.
///
/// This is the compatibility witness between a native engine and the runtime
/// (it replaces the former `TypioEngineInfo::struct_size`).
#[repr(C)]
pub struct TypioAbiVersion {
    pub major: u32,
    pub minor: u32,
}

/// Type of the `typio_engine_abi_version` entry point every plugin exports.
pub type TypioEngineAbiVersionFunc = unsafe extern "C" fn() -> *const TypioAbiVersion;

#[repr(C)]
pub struct TypioEngineInfo {
    pub name: *const c_char,
    pub display_name: *const c_char,
    pub description: *const c_char,
    pub author: *const c_char,
    pub icon: *const c_char,
    pub language: *const c_char,
    pub type_: TypioEngineType,
    /// NULL-terminated array of NUL-terminated capability name strings.
    /// May itself be NULL (treated as empty).
    pub required_capabilities: *const *const c_char,
    /// Same shape as `required_capabilities`; host logs but tolerates misses.
    pub optional_capabilities: *const *const c_char,
}

// SAFETY: All pointers point to immutable string literals used for C ABI.
unsafe impl Sync for TypioEngineInfo {}

#[repr(C)]
pub struct TypioEngine {
    pub info: *const TypioEngineInfo,
    pub base_ops: *const TypioEngineBaseOps,
    pub instance: *mut TypioInstance,
    pub user_data: *mut c_void,
    pub active: bool,
    pub initialized: bool,
    pub config_path: *mut c_char,
    pub surface: *const TypioEngineSurfaceOps,
}

#[repr(C)]
pub struct TypioKeyboardEngine {
    pub base: TypioEngine,
    pub keyboard: *const TypioKeyboardEngineOps,
}

#[repr(C)]
pub struct TypioVoiceEngine {
    pub base: TypioEngine,
    pub voice: *const TypioVoiceEngineOps,
}

#[repr(C)]
pub struct TypioEngineCommand {
    pub id: *const c_char,
    pub label: *const c_char,
}

/// Engine command surface (ADR-0008).
#[repr(C)]
pub struct TypioEngineSurfaceOps {
    pub list_commands:
        Option<extern "C" fn(*mut TypioEngine, *mut usize) -> *const TypioEngineCommand>,
    pub invoke_command: Option<extern "C" fn(*mut TypioEngine, *const c_char) -> TypioResult>,
}

pub type TypioKeyboardEngineFactory = unsafe extern "C" fn() -> *mut TypioKeyboardEngine;
pub type TypioVoiceEngineFactory = unsafe extern "C" fn() -> *mut TypioVoiceEngine;
pub type TypioEngineInfoFunc = unsafe extern "C" fn() -> *const TypioEngineInfo;

#[repr(C)]
pub struct TypioEngineBaseOps {
    pub init: Option<extern "C" fn(*mut TypioEngine, *mut TypioInstance) -> TypioResult>,
    pub destroy: Option<extern "C" fn(*mut TypioEngine)>,
    pub deactivate: Option<extern "C" fn(*mut TypioEngine)>,
    pub focus_in: Option<extern "C" fn(*mut TypioEngine, *mut TypioInputContext)>,
    pub focus_out: Option<extern "C" fn(*mut TypioEngine, *mut TypioInputContext)>,
    pub reset: Option<extern "C" fn(*mut TypioEngine, *mut TypioInputContext)>,
    pub reload_config: Option<extern "C" fn(*mut TypioEngine) -> TypioResult>,
    /// A configuration key the engine owns changed (ADR-0008).
    pub on_config_change: Option<extern "C" fn(*mut TypioEngine, *const c_char, *const c_char)>,
    /// Report current availability (ADR-0014). NULL means always Ready.
    pub availability: Option<extern "C" fn(*mut TypioEngine) -> TypioEngineAvailability>,
}

#[repr(C)]
pub struct TypioKeyboardEngineOps {
    pub process_key: Option<
        extern "C" fn(
            *mut TypioKeyboardEngine,
            *mut TypioInputContext,
            *const c_void,
        ) -> TypioKeyProcessResult,
    >,
    pub list_modes: Option<
        extern "C" fn(*mut TypioKeyboardEngine, *mut usize) -> *const TypioKeyboardEngineMode,
    >,
    pub get_active_mode: Option<
        extern "C" fn(
            *mut TypioKeyboardEngine,
            *mut TypioInputContext,
        ) -> *const TypioKeyboardEngineMode,
    >,
    pub set_active_mode: Option<
        extern "C" fn(
            *mut TypioKeyboardEngine,
            *mut TypioInputContext,
            *const c_char,
        ) -> TypioResult,
    >,
    pub commit_candidate:
        Option<extern "C" fn(*mut TypioKeyboardEngine, *mut TypioInputContext, i32) -> TypioResult>,
}

#[repr(C)]
pub struct TypioVoiceEngineOps {
    pub process_audio:
        Option<extern "C" fn(*mut TypioVoiceEngine, *const f32, usize) -> *mut c_char>,
}

/* -------------------------------------------------------------------------- */
/* Callback types                                                             */
/* -------------------------------------------------------------------------- */

pub type TypioCommitCallback = extern "C" fn(*mut TypioInputContext, *const c_char, *mut c_void);
pub type TypioCompositionCallback =
    extern "C" fn(*mut TypioInputContext, *const TypioComposition, *mut c_void);
/// Delete text around the cursor: `before` UTF-8 bytes preceding it and
/// `after` UTF-8 bytes following it (Wayland text-input v3 semantics).
pub type TypioDeleteSurroundingCallback =
    extern "C" fn(*mut TypioInputContext, u32, u32, *mut c_void);

#[repr(C)]
pub struct TypioLogEvent {
    pub level: TypioLogLevel,
    pub message: *const c_char,
    pub domain: *const c_char,
    pub file: *const c_char,
    pub line: u32,
    pub timestamp_ms: u64,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypioLogLevel {
    TypioLogTrace = 0,
    TypioLogDebug = 1,
    TypioLogInfo = 2,
    TypioLogWarning = 3,
    TypioLogError = 4,
}

pub type TypioLogCallback = extern "C" fn(*const TypioLogEvent, *mut c_void);

pub type TypioEngineChangedCallback =
    extern "C" fn(*mut TypioInstance, *const TypioEngineInfo, *mut c_void);
pub type TypioVoiceEngineChangedCallback =
    extern "C" fn(*mut TypioInstance, *const TypioEngineInfo, *mut c_void);
pub type TypioStatusIconChangedCallback =
    extern "C" fn(*mut TypioInstance, *const c_char, *mut c_void);
pub type TypioKeyboardModeChangedCallback =
    extern "C" fn(*mut TypioInstance, *const TypioKeyboardEngineMode, *mut c_void);
pub type TypioEngineAvailabilityChangedCallback =
    extern "C" fn(*mut TypioInstance, TypioEngineAvailability, *const c_char, *mut c_void);
pub type TypioLanguagesChangedCallback =
    extern "C" fn(*mut TypioInstance, *const c_char, *mut c_void);

/* -------------------------------------------------------------------------- */
/* Key symbol definitions (XKB compatible)                                    */
/* -------------------------------------------------------------------------- */

pub const TYPIO_KEY_BackSpace: u32 = 0xff08;
pub const TYPIO_KEY_Tab: u32 = 0xff09;
pub const TYPIO_KEY_Return: u32 = 0xff0d;
pub const TYPIO_KEY_KP_Enter: u32 = 0xff8d;
pub const TYPIO_KEY_Escape: u32 = 0xff1b;
pub const TYPIO_KEY_Delete: u32 = 0xffff;
pub const TYPIO_KEY_Home: u32 = 0xff50;
pub const TYPIO_KEY_Left: u32 = 0xff51;
pub const TYPIO_KEY_Up: u32 = 0xff52;
pub const TYPIO_KEY_Right: u32 = 0xff53;
pub const TYPIO_KEY_Down: u32 = 0xff54;
pub const TYPIO_KEY_Page_Up: u32 = 0xff55;
pub const TYPIO_KEY_Page_Down: u32 = 0xff56;
pub const TYPIO_KEY_End: u32 = 0xff57;
pub const TYPIO_KEY_space: u32 = 0x0020;

pub const TYPIO_KEY_Shift_L: u32 = 0xffe1;
pub const TYPIO_KEY_Shift_R: u32 = 0xffe2;
pub const TYPIO_KEY_Control_L: u32 = 0xffe3;
pub const TYPIO_KEY_Control_R: u32 = 0xffe4;
pub const TYPIO_KEY_Alt_L: u32 = 0xffe9;
pub const TYPIO_KEY_Alt_R: u32 = 0xffea;
pub const TYPIO_KEY_Super_L: u32 = 0xffeb;
pub const TYPIO_KEY_Super_R: u32 = 0xffec;

pub const TYPIO_KEY_F1: u32 = 0xffbe;
pub const TYPIO_KEY_F2: u32 = 0xffbf;
pub const TYPIO_KEY_F3: u32 = 0xffc0;
pub const TYPIO_KEY_F4: u32 = 0xffc1;
pub const TYPIO_KEY_F5: u32 = 0xffc2;
pub const TYPIO_KEY_F6: u32 = 0xffc3;
pub const TYPIO_KEY_F7: u32 = 0xffc4;
pub const TYPIO_KEY_F8: u32 = 0xffc5;
pub const TYPIO_KEY_F9: u32 = 0xffc6;
pub const TYPIO_KEY_F10: u32 = 0xffc7;
pub const TYPIO_KEY_F11: u32 = 0xffc8;
pub const TYPIO_KEY_F12: u32 = 0xffc9;
