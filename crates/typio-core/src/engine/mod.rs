//! Engine-mode metadata helpers.
//!
//! Engine runtime is process-only. This module no longer exposes C-vtable
//! construction/destruction helpers for in-process engines.

use crate::string::typio_strdup;
use crate::types::TypioKeyboardEngineMode;
use std::ffi::{CStr, c_char};
use std::ptr;

/// Compare two engine modes by identity: mode id (ADR-0011).
pub(crate) fn engine_mode_equal(a: &TypioKeyboardEngineMode, b: &TypioKeyboardEngineMode) -> bool {
    let a_id = unsafe { a.id.as_ref() }.and_then(|p| unsafe { CStr::from_ptr(p) }.to_str().ok());
    let b_id = unsafe { b.id.as_ref() }.and_then(|p| unsafe { CStr::from_ptr(p) }.to_str().ok());
    a_id == b_id
}

/// Deep-copy mode metadata into `dst`, freeing any existing strings first.
pub(crate) fn engine_mode_store(dst: &mut TypioKeyboardEngineMode, src: &TypioKeyboardEngineMode) {
    crate::string::typio_free_string(dst.id as *mut c_char);
    crate::string::typio_free_string(dst.label as *mut c_char);
    crate::string::typio_free_string(dst.display_label as *mut c_char);
    crate::string::typio_free_string(dst.icon_name as *mut c_char);
    crate::string::typio_free_string(dst.profile_id as *mut c_char);
    crate::string::typio_free_string(dst.profile_label as *mut c_char);
    crate::string::typio_free_string(dst.description as *mut c_char);

    dst.id = if src.id.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.id)
    };
    dst.label = if src.label.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.label)
    };
    dst.display_label = if src.display_label.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.display_label)
    };
    dst.icon_name = if src.icon_name.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.icon_name)
    };
    dst.profile_id = if src.profile_id.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.profile_id)
    };
    dst.profile_label = if src.profile_label.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.profile_label)
    };
    dst.description = if src.description.is_null() {
        ptr::null()
    } else {
        typio_strdup(src.description)
    };
    dst.salience = src.salience;
}
