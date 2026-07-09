use crate::candidate_guard::HostSelectionFlags;
use std::ffi::{CStr, c_char, c_void};
use typio_abi::TypioComposition;

/// Engine output staged by libtypio callbacks during `process_key`.
#[derive(Default)]
pub(super) struct PendingEngineOutput {
    pub commit: Option<String>,
    pub composition: Option<PendingComposition>,
}

/// Pending composition state since the last key dispatch. Staged by the
/// engine's composition callback on the same thread that called
/// `typio_input_context_process_key`; drained by `drain_composition`.
///
/// `cursor_pos` is the engine's requested byte offset into `preedit_text`
/// (negative means "place at the end"); see [`crate::preedit::resolve_cursor`].
/// `host_managed_selection` carries the engine's declared
/// selection-intercept flags (ADR-0012) so the host can apply
/// [`crate::candidate_guard`] without a separate engine→host round-trip.
#[derive(Default)]
pub(super) struct PendingComposition {
    pub preedit_text: String,
    pub cursor_pos: i32,
    pub candidates: Vec<String>,
    pub selected: usize,
    pub has_prev: bool,
    pub has_next: bool,
    pub host_managed_selection: HostSelectionFlags,
}

pub(super) extern "C" fn on_commit_abi(
    ctx: *mut typio_abi::TypioInputContext,
    text: *const c_char,
    user_data: *mut c_void,
) {
    on_commit(ctx as *mut typio::TypioInputContext, text, user_data)
}

pub(super) extern "C" fn on_commit(
    _ctx: *mut typio::TypioInputContext,
    text: *const c_char,
    user_data: *mut c_void,
) {
    if text.is_null() || user_data.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        (*(user_data as *mut PendingEngineOutput)).commit = Some(s);
    }
}

pub(super) extern "C" fn on_composition_abi(
    ctx: *mut typio_abi::TypioInputContext,
    comp: *const TypioComposition,
    user_data: *mut c_void,
) {
    on_composition(ctx as *mut typio::TypioInputContext, comp, user_data)
}

extern "C" fn on_composition(
    _ctx: *mut typio::TypioInputContext,
    comp: *const TypioComposition,
    user_data: *mut c_void,
) {
    if comp.is_null() || user_data.is_null() {
        return;
    }
    let comp = unsafe { &*comp };

    let mut candidates = Vec::new();
    if !comp.candidates.is_null() && comp.candidate_count > 0 {
        for i in 0..comp.candidate_count {
            let c = unsafe { &*comp.candidates.add(i) };
            if !c.text.is_null() {
                let text = unsafe { CStr::from_ptr(c.text) }
                    .to_string_lossy()
                    .into_owned();
                candidates.push(text);
            }
        }
    }

    let mut preedit = String::new();
    if !comp.segments.is_null() && comp.segment_count > 0 {
        for i in 0..comp.segment_count {
            let seg = unsafe { &*comp.segments.add(i) };
            if !seg.text.is_null() {
                preedit.push_str(&unsafe { CStr::from_ptr(seg.text) }.to_string_lossy());
            }
        }
    }

    let selected = comp.selected.max(0) as usize;
    let cursor_pos = comp.cursor_pos;
    let host_managed_selection =
        HostSelectionFlags::from_bits_truncate(comp.host_managed_selection);

    unsafe {
        (*(user_data as *mut PendingEngineOutput)).composition = Some(PendingComposition {
            preedit_text: preedit,
            cursor_pos,
            candidates,
            selected,
            has_prev: comp.has_prev,
            has_next: comp.has_next,
            host_managed_selection,
        });
    }
}
