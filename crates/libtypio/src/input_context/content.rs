//! Composition (preedit + candidates) and surrounding text management.
//!
//! ADR-0006: composition is one transactional value emitted via
//! `set_composition`; commit is a separate ordered event. `get_preedit` /
//! `get_candidates` remain as read projections of the stored composition.

use super::{
    candidate_signature, TypioCandidate, TypioComposition, TypioInputContext, TypioPreedit,
    TypioPreeditSegment,
};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

/// Compare two NUL-terminated C strings for equality, treating NULL as equal to NULL.
fn c_str_eq(a: *const c_char, b: *const c_char) -> bool {
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    unsafe { CStr::from_ptr(a) == CStr::from_ptr(b) }
}

/// Check whether the candidate content (everything except `selected`) is
/// identical between the current stored state and the incoming composition.
/// This is the fast-path guard that lets us skip a full clear+rebuild when
/// the user simply moved the highlight (Up/Down) without changing the page.
fn candidates_content_unchanged(ctx: &TypioInputContext, comp: &TypioComposition) -> bool {
    if ctx.candidates.count != comp.candidate_count {
        return false;
    }
    if ctx.candidates.page != comp.page
        || ctx.candidates.page_size != comp.page_size
        || ctx.candidates.total != comp.total
        || ctx.candidates.has_prev != comp.has_prev
        || ctx.candidates.has_next != comp.has_next
    {
        return false;
    }
    if comp.candidate_count == 0 {
        return true;
    }
    if comp.candidates.is_null() || ctx.candidates.candidates.is_null() {
        return false;
    }
    let old_slice =
        unsafe { std::slice::from_raw_parts(ctx.candidates.candidates, ctx.candidates.count) };
    let new_slice = unsafe { std::slice::from_raw_parts(comp.candidates, comp.candidate_count) };
    for (o, n) in old_slice.iter().zip(new_slice.iter()) {
        if !c_str_eq(o.text, n.text)
            || !c_str_eq(o.comment, n.comment)
            || !c_str_eq(o.label, n.label)
        {
            return false;
        }
    }
    true
}

/// Check whether the preedit content and formatting is unchanged.
fn preedit_unchanged(ctx: &TypioInputContext, comp: &TypioComposition) -> bool {
    if ctx.preedit.segment_count != comp.segment_count {
        return false;
    }
    if ctx.preedit.segment_count == 0 {
        return true;
    }
    if comp.segments.is_null() || ctx.preedit.segments.is_null() {
        return false;
    }
    let old_slice =
        unsafe { std::slice::from_raw_parts(ctx.preedit.segments, ctx.preedit.segment_count) };
    let new_slice = unsafe { std::slice::from_raw_parts(comp.segments, comp.segment_count) };
    for (o, n) in old_slice.iter().zip(new_slice.iter()) {
        if !c_str_eq(o.text, n.text) || o.format != n.format {
            return false;
        }
    }
    true
}

/// Commit the given text and clear the composition.
#[no_mangle]
pub extern "C" fn typio_input_context_commit(ctx: *mut TypioInputContext, text: *const c_char) {
    if ctx.is_null() || text.is_null() {
        return;
    }

    let ctx_ref = unsafe { &mut *ctx };

    // Commit clears the in-flight composition. It is silent: the commit
    // callback owns the resulting UI (clear preedit on the wire, hide popup),
    // so we do not also fire a composition update.
    ctx_ref.clear_preedit_silent();
    ctx_ref.clear_candidates_silent();
    ctx_ref.revision = ctx_ref.revision.wrapping_add(1);

    if let Some(cb) = ctx_ref.commit_callback {
        cb(ctx.cast(), text, ctx_ref.commit_user_data);
    }
}

/// Set the entire in-flight composition (preedit + candidates) atomically and
/// fire the composition callback once. An empty composition is the Idle state.
#[no_mangle]
pub extern "C" fn typio_input_context_set_composition(
    ctx: *mut TypioInputContext,
    comp: *const TypioComposition,
) {
    if ctx.is_null() || comp.is_null() {
        return;
    }
    let ctx_ref = unsafe { &mut *ctx };
    let c = unsafe { &*comp };

    // Fast path: only `selected` or `cursor_pos` changed (e.g. Up/Down
    // navigation inside the same candidate page).  Skip the expensive
    // clear+rebuild and all associated CString allocations.
    if candidates_content_unchanged(ctx_ref, c) && preedit_unchanged(ctx_ref, c) {
        ctx_ref.candidates.selected = c.selected;
        ctx_ref.preedit.cursor_pos = c.cursor_pos;
        // Signature excludes `selected` by design, so it is unchanged.
        ctx_ref.emit_composition(ctx);
        return;
    }

    ctx_ref.clear_preedit_silent();
    ctx_ref.clear_candidates_silent();

    // Preedit segments.
    let segs = if c.segment_count > 0 && !c.segments.is_null() {
        unsafe { std::slice::from_raw_parts(c.segments, c.segment_count) }
    } else {
        &[]
    };
    ctx_ref.preedit_segments.reserve(segs.len());
    for seg in segs {
        let text_copy = if seg.text.is_null() {
            ptr::null()
        } else {
            unsafe { CStr::from_ptr(seg.text) }.to_owned().into_raw() as *const c_char
        };
        ctx_ref.preedit_segments.push(TypioPreeditSegment {
            text: text_copy,
            format: seg.format,
        });
    }
    ctx_ref.preedit.segment_count = ctx_ref.preedit_segments.len();
    ctx_ref.preedit.cursor_pos = c.cursor_pos;
    ctx_ref.preedit.segments = ctx_ref.preedit_segments.as_mut_ptr();

    // Candidates.
    let cands = if c.candidate_count > 0 && !c.candidates.is_null() {
        unsafe { std::slice::from_raw_parts(c.candidates, c.candidate_count) }
    } else {
        &[]
    };
    ctx_ref.candidate_items.reserve(cands.len());
    for cand in cands {
        ctx_ref.candidate_items.push(TypioCandidate {
            text: if cand.text.is_null() {
                ptr::null()
            } else {
                unsafe { CStr::from_ptr(cand.text) }.to_owned().into_raw() as *const c_char
            },
            comment: if cand.comment.is_null() {
                ptr::null()
            } else {
                unsafe { CStr::from_ptr(cand.comment) }
                    .to_owned()
                    .into_raw() as *const c_char
            },
            label: if cand.label.is_null() {
                ptr::null()
            } else {
                unsafe { CStr::from_ptr(cand.label) }.to_owned().into_raw() as *const c_char
            },
        });
    }
    ctx_ref.candidates.count = ctx_ref.candidate_items.len();
    ctx_ref.candidates.page = c.page;
    ctx_ref.candidates.page_size = c.page_size;
    ctx_ref.candidates.total = c.total;
    ctx_ref.candidates.selected = c.selected;
    ctx_ref.candidates.has_prev = c.has_prev;
    ctx_ref.candidates.has_next = c.has_next;
    ctx_ref.candidates.candidates = ctx_ref.candidate_items.as_mut_ptr();
    ctx_ref.candidates.content_signature = candidate_signature(&ctx_ref.candidates);
    ctx_ref.candidates.host_managed_selection = c.host_managed_selection;

    ctx_ref.emit_composition(ctx);
}

/// Clear the composition to the Idle state (empty preedit + candidates) and
/// fire the composition callback.
#[no_mangle]
pub extern "C" fn typio_input_context_clear(ctx: *mut TypioInputContext) {
    if ctx.is_null() {
        return;
    }
    let ctx_ref = unsafe { &mut *ctx };
    ctx_ref.clear_preedit_silent();
    ctx_ref.clear_candidates_silent();
    ctx_ref.emit_composition(ctx);
}

/// Get the current preedit state.
#[no_mangle]
pub extern "C" fn typio_input_context_get_preedit(
    ctx: *mut TypioInputContext,
) -> *const TypioPreedit {
    if ctx.is_null() {
        return ptr::null();
    }
    unsafe { &(*ctx).preedit }
}

/// Set the surrounding text and cursor positions.
#[no_mangle]
pub extern "C" fn typio_input_context_set_surrounding(
    ctx: *mut TypioInputContext,
    text: *const c_char,
    cursor_pos: i32,
    anchor_pos: i32,
) {
    if ctx.is_null() {
        return;
    }
    let ctx_ref = unsafe { &mut *ctx };

    ctx_ref.surrounding_text = if text.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(text) }.to_owned())
    };
    ctx_ref.surrounding_cursor = cursor_pos;
    ctx_ref.surrounding_anchor = anchor_pos;
}

/// Get the surrounding text and cursor positions.
#[no_mangle]
pub extern "C" fn typio_input_context_get_surrounding(
    ctx: *mut TypioInputContext,
    text: *mut *const c_char,
    cursor_pos: *mut i32,
    anchor_pos: *mut i32,
) -> bool {
    if ctx.is_null() {
        return false;
    }
    let ctx_ref = unsafe { &*ctx };

    if let Some(ref s) = ctx_ref.surrounding_text {
        if !text.is_null() {
            unsafe { *text = s.as_ptr() };
        }
        if !cursor_pos.is_null() {
            unsafe { *cursor_pos = ctx_ref.surrounding_cursor };
        }
        if !anchor_pos.is_null() {
            unsafe { *anchor_pos = ctx_ref.surrounding_anchor };
        }
        true
    } else {
        false
    }
}

/// Request the client delete text around the cursor: `before` UTF-8 bytes
/// preceding the cursor and `after` bytes following it (Wayland text-input v3
/// `delete_surrounding_text` semantics). Engines use this to erase committed
/// context — e.g. backspacing into an already-committed phrase.
///
/// Fires the registered delete-surrounding callback (the host forwards it to
/// the focused client) and best-effort updates the cached surrounding text so
/// subsequent in-flight reads stay consistent before the client echoes back.
#[no_mangle]
pub extern "C" fn typio_input_context_delete_surrounding(
    ctx: *mut TypioInputContext,
    before: u32,
    after: u32,
) {
    if ctx.is_null() || (before == 0 && after == 0) {
        return;
    }
    let ctx_ref = unsafe { &mut *ctx };

    // Best-effort local cache update: byte-trim around the cursor so a
    // following get_surrounding reflects the request. The authoritative state
    // arrives when the client sends a fresh set_surrounding.
    if let Some(ref s) = ctx_ref.surrounding_text {
        let bytes = s.as_bytes();
        let cursor = ctx_ref.surrounding_cursor.clamp(0, bytes.len() as i32) as usize;
        let del_before = (before as usize).min(cursor);
        let start = cursor - del_before;
        let end = (cursor + after as usize).min(bytes.len());
        if start < end {
            let mut kept = Vec::with_capacity(bytes.len() - (end - start));
            kept.extend_from_slice(&bytes[..start]);
            kept.extend_from_slice(&bytes[end..]);
            if let Ok(updated) = CString::new(kept) {
                ctx_ref.surrounding_text = Some(updated);
                let new_cursor = start as i32;
                ctx_ref.surrounding_cursor = new_cursor;
                ctx_ref.surrounding_anchor = new_cursor;
            }
        }
    }

    if let Some(cb) = ctx_ref.delete_surrounding_callback {
        cb(
            ctx.cast(),
            before,
            after,
            ctx_ref.delete_surrounding_user_data,
        );
    }
}
