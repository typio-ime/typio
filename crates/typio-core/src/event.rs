//! Event handling — Rust implementation of event.c

#![allow(non_upper_case_globals)]

use crate::types::*;
use std::ffi::{c_int, c_void};
use std::ptr;

/// Allocate a new key event.
#[no_mangle]
pub extern "C" fn typio_key_event_new(
    type_: TypioEventType,
    keycode: u32,
    keysym: u32,
    modifiers: u32,
) -> *mut TypioKeyEvent {
    let event = Box::new(TypioKeyEvent {
        struct_size: std::mem::size_of::<TypioKeyEvent>(),
        type_,
        keycode,
        keysym,
        modifiers,
        unicode: 0,
        time: 0,
        is_repeat: false,
        base_keysym: 0,
    });
    Box::into_raw(event)
}

/// Free a key event allocated by `typio_key_event_new`.
#[no_mangle]
pub extern "C" fn typio_key_event_free(event: *mut TypioKeyEvent) {
    if !event.is_null() {
        unsafe { drop(Box::from_raw(event)) };
    }
}

/// Return true if the event is a key press.
#[no_mangle]
pub extern "C" fn typio_key_event_is_press(event: *const TypioKeyEvent) -> bool {
    if event.is_null() {
        return false;
    }
    unsafe { (*event).type_ == TypioEventType::TypioEventKeyPress }
}

/// Return true if the event is a key release.
#[no_mangle]
pub extern "C" fn typio_key_event_is_release(event: *const TypioKeyEvent) -> bool {
    if event.is_null() {
        return false;
    }
    unsafe { (*event).type_ == TypioEventType::TypioEventKeyRelease }
}

/// Return true if the given modifier is active in the event.
#[no_mangle]
pub extern "C" fn typio_key_event_has_modifier(
    event: *const TypioKeyEvent,
    mod_: TypioModifier,
) -> bool {
    if event.is_null() {
        return false;
    }
    unsafe { ((*event).modifiers & (mod_ as u32)) != 0 }
}

/// Return true if the event is a pure modifier key (no printable character).
#[no_mangle]
pub extern "C" fn typio_key_event_is_modifier_only(event: *const TypioKeyEvent) -> bool {
    if event.is_null() {
        return false;
    }
    let keysym = unsafe { (*event).keysym };
    matches!(
        keysym,
        TYPIO_KEY_Shift_L
            | TYPIO_KEY_Shift_R
            | TYPIO_KEY_Control_L
            | TYPIO_KEY_Control_R
            | TYPIO_KEY_Alt_L
            | TYPIO_KEY_Alt_R
            | TYPIO_KEY_Super_L
            | TYPIO_KEY_Super_R
    )
}

/// Return the Unicode codepoint for the event, or 0 if none.
#[no_mangle]
pub extern "C" fn typio_key_event_get_unicode(event: *const TypioKeyEvent) -> u32 {
    if event.is_null() {
        return 0;
    }
    let ev = unsafe { &*event };
    if ev.unicode != 0 {
        return ev.unicode;
    }
    if ev.keysym >= 0x20 && ev.keysym <= 0x7e {
        return ev.keysym;
    }
    0
}

/// Return true if the event is BackSpace.
#[no_mangle]
pub extern "C" fn typio_key_event_is_backspace(event: *const TypioKeyEvent) -> bool {
    !event.is_null() && unsafe { (*event).keysym == TYPIO_KEY_BackSpace }
}

/// Return true if the event is Enter/Return.
#[no_mangle]
pub extern "C" fn typio_key_event_is_enter(event: *const TypioKeyEvent) -> bool {
    !event.is_null() && unsafe { (*event).keysym == TYPIO_KEY_Return }
}

/// Return true if the event is Escape.
#[no_mangle]
pub extern "C" fn typio_key_event_is_escape(event: *const TypioKeyEvent) -> bool {
    !event.is_null() && unsafe { (*event).keysym == TYPIO_KEY_Escape }
}

/// Return true if the event is Space.
#[no_mangle]
pub extern "C" fn typio_key_event_is_space(event: *const TypioKeyEvent) -> bool {
    !event.is_null() && unsafe { (*event).keysym == TYPIO_KEY_space }
}

/// Return true if the event is Tab.
#[no_mangle]
pub extern "C" fn typio_key_event_is_tab(event: *const TypioKeyEvent) -> bool {
    !event.is_null() && unsafe { (*event).keysym == TYPIO_KEY_Tab }
}

/// Return true if the event is an arrow key.
#[no_mangle]
pub extern "C" fn typio_key_event_is_arrow(event: *const TypioKeyEvent) -> bool {
    if event.is_null() {
        return false;
    }
    let keysym = unsafe { (*event).keysym };
    matches!(
        keysym,
        TYPIO_KEY_Left | TYPIO_KEY_Right | TYPIO_KEY_Up | TYPIO_KEY_Down
    )
}

/// Return true if the event is Page Up or Page Down.
#[no_mangle]
pub extern "C" fn typio_key_event_is_page(event: *const TypioKeyEvent) -> bool {
    if event.is_null() {
        return false;
    }
    let keysym = unsafe { (*event).keysym };
    matches!(keysym, TYPIO_KEY_Page_Up | TYPIO_KEY_Page_Down)
}

/// Allocate a new voice event.
#[no_mangle]
pub extern "C" fn typio_voice_event_new(type_: TypioEventType) -> *mut TypioVoiceEvent {
    let event = Box::new(TypioVoiceEvent {
        type_,
        audio_data: ptr::null(),
        audio_size: 0,
        sample_rate: 0,
        channels: 0,
        bits_per_sample: 0,
    });
    Box::into_raw(event)
}

/// Free a voice event allocated by `typio_voice_event_new`.
#[no_mangle]
pub extern "C" fn typio_voice_event_free(event: *mut TypioVoiceEvent) {
    if !event.is_null() {
        unsafe { drop(Box::from_raw(event)) };
    }
}

/// Attach audio data to a voice event.
#[no_mangle]
pub extern "C" fn typio_voice_event_set_data(
    event: *mut TypioVoiceEvent,
    data: *const c_void,
    size: usize,
    sample_rate: c_int,
    channels: c_int,
    bits_per_sample: c_int,
) {
    if event.is_null() {
        return;
    }
    let ev = unsafe { &mut *event };
    ev.audio_data = data;
    ev.audio_size = size;
    ev.sample_rate = sample_rate;
    ev.channels = channels;
    ev.bits_per_sample = bits_per_sample;
}
