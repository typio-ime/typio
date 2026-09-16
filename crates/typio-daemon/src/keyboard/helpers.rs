use crate::keyboard_policy::{KEY_CAPITAL_V, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_V};
use typio_host_types::Modifiers;

pub(super) fn is_voice_ptt_key(keysym: u32) -> bool {
    keysym == KEY_V || keysym == KEY_CAPITAL_V
}

pub(super) fn page_boundary_selected_index(keysym: u32, candidate_count: usize) -> Option<usize> {
    if candidate_count == 0 {
        return None;
    }
    if keysym == KEY_PAGE_UP {
        Some(candidate_count - 1)
    } else if keysym == KEY_PAGE_DOWN {
        Some(0)
    } else {
        None
    }
}

pub(super) fn commit_candidate_should_fallback(
    result: &typio_runtime::core::engine::Result<()>,
    produced_engine_output: bool,
) -> bool {
    result.is_err() && !produced_engine_output
}

pub(super) fn host_selection_plain_key(modifiers: Modifiers) -> bool {
    let selection_modifiers =
        Modifiers::SHIFT.0 | Modifiers::CTRL.0 | Modifiers::ALT.0 | Modifiers::SUPER.0;
    (modifiers.0 & selection_modifiers) == 0
}

pub(super) fn should_suppress_untracked_modifier_release(
    bit: Modifiers,
    effective_held: Modifiers,
) -> bool {
    if bit == Modifiers::SHIFT {
        let blocking = Modifiers(Modifiers::CTRL.0 | Modifiers::ALT.0 | Modifiers::SUPER.0);
        effective_held.intersects(blocking)
    } else {
        true
    }
}
