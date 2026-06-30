/**
 * @file shortcut.h
 * @brief Configurable keyboard shortcut bindings.
 *
 * Bindings are addressed by **action ID** (a stable snake_case string) so
 * that adding a new shortcut is purely additive — no struct-layout change.
 *
 * Standard action IDs:
 *
 *   - `switch_language`         (default: Ctrl+Shift)
 *   - `exit`                    (default: Ctrl+Shift+Escape)
 *   - `voice_ptt`               (default: Super+v)
 *   - `summon_indicator`        (default: Ctrl+Super+i)
 *
 * `switch_keyboard_engine` remains a recognized action ID but has no
 * built-in default since the language model (ADR-0018) took over the
 * Ctrl+Shift chord; bind it via `shortcuts.switch_keyboard_engine`.
 *
 * `summon_indicator` actively re-shows the on-screen indicator (language ·
 * engine · mode) on demand. It only fires while a text field is focused —
 * the indicator uses `zwp_input_popup_surface_v2`, which is bound to the
 * active input-method session.
 *
 * Hosts look up a binding with `typio_shortcut_get`, which consults the
 * config first (`shortcuts.<action_id>`) and falls back to the built-in
 * default.
 */

#ifndef TYPIO_SHORTCUT_H
#define TYPIO_SHORTCUT_H

#include "typio/abi/types.h"

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief A single shortcut binding: modifier mask + optional keysym.
 *
 * If keysym == 0, this is a modifier-only chord (e.g. Ctrl+Shift).
 */
typedef struct {
    uint32_t modifiers;     /* TYPIO_MOD_* bitmask */
    uint32_t keysym;        /* XKB keysym, or 0 for modifier-only */
} TypioShortcutBinding;

/**
 * @brief Resolve the binding for a named action.
 *
 * Reads `shortcuts.<action_id>` from @p config; on miss or parse failure
 * falls back to `typio_shortcut_default(action_id, out)`. Returns true if
 * `*out` was written.
 */
bool typio_shortcut_get(const TypioConfig *config,
                        const char *action_id,
                        TypioShortcutBinding *out);

/**
 * @brief Look up the built-in default binding for a known action ID.
 *
 * Returns false for unknown IDs (and leaves `*out` untouched).
 */
bool typio_shortcut_default(const char *action_id,
                            TypioShortcutBinding *out);

/**
 * @brief Parse a shortcut string like "Ctrl+Shift" or "Super+v" into a
 *        binding. Returns true on success.
 */
bool typio_shortcut_parse(const char *str, TypioShortcutBinding *out);

/**
 * @brief Format a binding back to a human-readable string.
 *
 * Returns a newly-allocated NUL-terminated string. Caller frees with
 * `typio_free_string`.
 */
char *typio_shortcut_format(const TypioShortcutBinding *binding);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_SHORTCUT_H */
