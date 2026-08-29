//! Host-wide keyboard modifier bitmask.
//!
//! One shared bit layout for every modifier decision in the host: the
//! platform layer maps the compositor's keymap-dependent xkb mask onto these
//! bits once (see `InputMethodState::effective_modifiers`), and every
//! downstream consumer — engine modifier mask, switch-chord detection, repeat
//! gating — works in this layout only. Never mix raw xkb wire bits into this
//! type: on a conventional keymap xkb puts NumLock at `1 << 4` and Super at
//! `1 << 6`, which does not match the constants below.

/// Bit flags for keyboard modifiers, mirroring the C `TYPIO_MOD_*`
/// constants used by the repeat gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct Modifiers(pub u32);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CAPSLOCK: Self = Self(1 << 1);
    pub const CTRL: Self = Self(1 << 2);
    pub const ALT: Self = Self(1 << 3);
    pub const SUPER: Self = Self(1 << 4);
    pub const NUMLOCK: Self = Self(1 << 5);

    /// True iff any of the given modifier bits is set.
    pub fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

/// Bit set of modifiers that suppress auto-repeat when held.
/// Matches the C macro `(TYPIO_MOD_CTRL | TYPIO_MOD_ALT | TYPIO_MOD_SUPER)`.
const REPEAT_SUPPRESSING_MODIFIERS: Modifiers =
    Modifiers(Modifiers::CTRL.0 | Modifiers::ALT.0 | Modifiers::SUPER.0);

/// Pure decision: should auto-repeat fire for a keypress with these
/// modifiers held?
///
/// Returns false when any of Ctrl, Alt, or Super is held. Matches the C
/// `keyboard_repeat_should_run` predicate.
pub fn should_repeat_for_modifiers(modifiers: Modifiers) -> bool {
    !modifiers.intersects(REPEAT_SUPPRESSING_MODIFIERS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_intersect_works() {
        assert!(Modifiers::CTRL.intersects(Modifiers(Modifiers::CTRL.0 | Modifiers::ALT.0)));
        assert!(!Modifiers::SHIFT.intersects(Modifiers::CTRL));
        assert!(!Modifiers::NONE.intersects(Modifiers::NONE));
    }

    #[test]
    fn should_repeat_returns_true_for_plain_keys() {
        assert!(should_repeat_for_modifiers(Modifiers::NONE));
        assert!(should_repeat_for_modifiers(Modifiers::SHIFT));
        assert!(should_repeat_for_modifiers(Modifiers::CAPSLOCK));
        assert!(should_repeat_for_modifiers(Modifiers::NUMLOCK));
        assert!(should_repeat_for_modifiers(Modifiers(
            Modifiers::SHIFT.0 | Modifiers::CAPSLOCK.0
        )));
    }

    #[test]
    fn should_repeat_returns_false_when_ctrl_alt_or_super_held() {
        assert!(!should_repeat_for_modifiers(Modifiers::CTRL));
        assert!(!should_repeat_for_modifiers(Modifiers::ALT));
        assert!(!should_repeat_for_modifiers(Modifiers::SUPER));
        // Any combination that includes a suppressor still suppresses.
        assert!(!should_repeat_for_modifiers(Modifiers(
            Modifiers::SHIFT.0 | Modifiers::CTRL.0
        )));
        assert!(!should_repeat_for_modifiers(Modifiers(
            Modifiers::CTRL.0 | Modifiers::ALT.0 | Modifiers::SUPER.0
        )));
    }
}
