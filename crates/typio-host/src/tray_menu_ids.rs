//! Shared dbusmenu item ID scheme for tray menu build/decode.

/// Restart/Quit/separator section.
pub const SECTION_MISC: i32 = 1000;
/// Per-language entries.
pub const SECTION_LANG: i32 = 2000;
/// Per-engine entries inside language submenus.
pub const SECTION_ENGINE: i32 = 3000;
/// Engines that declare no registered language.
pub const SECTION_ORPHAN: i32 = 4000;
/// Voice engine entries.
pub const SECTION_VOICE: i32 = 5000;

/// Maximum language menu entries encoded in the fixed ID range.
pub const LANG_MAX: usize = 16;
/// Maximum keyboard entries encoded per language.
pub const ENGINE_MAX: usize = 16;
/// Maximum orphan keyboard entries encoded in the fixed ID range.
pub const ORPHAN_MAX: usize = 16;
/// Maximum voice entries encoded in the fixed ID range.
pub const VOICE_MAX: usize = 16;

/// Restart item ID.
pub const ITEM_RESTART: i32 = SECTION_MISC + 1;
/// Quit item ID.
pub const ITEM_QUIT: i32 = SECTION_MISC + 2;
/// First separator item ID.
pub const ITEM_SEP_BEGIN: i32 = SECTION_MISC + 100;

/// Decoded tray menu action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Restart the daemon.
    Restart,
    /// Quit the daemon.
    Quit,
    /// Switch active language by visible language index.
    Language(i32),
    /// Switch to the engine at `engine_idx` as shown under `lang_idx`.
    EngineInLanguage { lang_idx: i32, engine_idx: i32 },
    /// Switch to an engine not attached to any language menu.
    OrphanEngine(i32),
    /// Switch active voice engine by visible index.
    Voice(i32),
    /// Unknown or out-of-range menu ID.
    Unknown,
}

/// Composite ID for an engine appearing under a language submenu.
pub fn engine_in_lang(lang_idx: usize, engine_idx: usize) -> i32 {
    SECTION_ENGINE + (lang_idx as i32) * (ENGINE_MAX as i32) + (engine_idx as i32)
}

/// Decode a dbusmenu item ID into the action it represents.
pub fn decode_menu_click(id: i32) -> MenuAction {
    let lang_max = LANG_MAX as i32;
    let engine_max = ENGINE_MAX as i32;
    let orphan_max = ORPHAN_MAX as i32;
    let voice_max = VOICE_MAX as i32;
    let engine_in_lang_max = lang_max * engine_max;

    if id == ITEM_RESTART {
        return MenuAction::Restart;
    }
    if id == ITEM_QUIT {
        return MenuAction::Quit;
    }
    if (SECTION_ENGINE..SECTION_ENGINE + engine_in_lang_max).contains(&id) {
        let offset = id - SECTION_ENGINE;
        return MenuAction::EngineInLanguage {
            lang_idx: offset / engine_max,
            engine_idx: offset % engine_max,
        };
    }
    if (SECTION_ORPHAN..SECTION_ORPHAN + orphan_max).contains(&id) {
        return MenuAction::OrphanEngine(id - SECTION_ORPHAN);
    }
    if (SECTION_LANG..SECTION_LANG + lang_max).contains(&id) {
        return MenuAction::Language(id - SECTION_LANG);
    }
    if (SECTION_VOICE..SECTION_VOICE + voice_max).contains(&id) {
        return MenuAction::Voice(id - SECTION_VOICE);
    }
    MenuAction::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_fixed_misc_actions() {
        assert_eq!(decode_menu_click(ITEM_RESTART), MenuAction::Restart);
        assert_eq!(decode_menu_click(ITEM_QUIT), MenuAction::Quit);
    }

    #[test]
    fn decode_language_section() {
        assert_eq!(decode_menu_click(SECTION_LANG), MenuAction::Language(0));
        assert_eq!(
            decode_menu_click(SECTION_LANG + LANG_MAX as i32 - 1),
            MenuAction::Language(LANG_MAX as i32 - 1)
        );
        assert_eq!(
            decode_menu_click(SECTION_LANG + LANG_MAX as i32),
            MenuAction::Unknown
        );
    }

    #[test]
    fn decode_engine_in_language_uses_composite_formula() {
        let id = engine_in_lang(2, 5);
        assert_eq!(
            decode_menu_click(id),
            MenuAction::EngineInLanguage {
                lang_idx: 2,
                engine_idx: 5
            }
        );
    }
}
