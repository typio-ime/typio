/**
 * @file registry.h
 * @brief Engine registry — the sole C surface for managing engines (ADR-0005).
 *
 * Hosts (`typiod-wayland`, the control panel, and future platform hosts)
 * use these functions to register, list, activate, and switch engines.
 *
 * @note This is a runtime header (no ABI stability promise). Engine plugins
 *       must NOT include this file; they pull only from `typio/abi/`.
 */

#ifndef TYPIO_REGISTRY_H
#define TYPIO_REGISTRY_H

#include "typio/abi/engine.h"
#include "typio/abi/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Opaque registry handle. */
typedef struct TypioRegistry TypioRegistry;

/* -------------------------------------------------------------------------- */
/* Lifecycle                                                                  */
/* -------------------------------------------------------------------------- */

/**
 * @brief Create a new engine registry.
 * @param instance Parent Typio instance (may be NULL for tests).
 * @return New registry or NULL on failure.
 */
TypioRegistry *typio_registry_new(TypioInstance *instance);

/** Return the parent instance, or NULL. */
TypioInstance *typio_registry_get_instance(TypioRegistry *registry);

/** Destroy registry and unload all engines. */
void typio_registry_free(TypioRegistry *registry);

/* -------------------------------------------------------------------------- */
/* Engine process registration                                                */
/* -------------------------------------------------------------------------- */

/**
 * @brief Register an out-of-process engine process.
 *
 * @param registry Engine registry.
 * @param info Engine metadata copied during the call.
 * @param argv NULL-terminated argument vector. `argv[0]` is the engine
 *        executable path.
 */
TypioResult typio_registry_register_engine_process(
    TypioRegistry *registry,
    const TypioEngineInfo *info,
    const char *const *argv);

/* -------------------------------------------------------------------------- */
/* Unload                                                                     */
/* -------------------------------------------------------------------------- */

/** Unload an engine by name. */
TypioResult typio_registry_unload(TypioRegistry *registry, const char *name);

/* -------------------------------------------------------------------------- */
/* Listing                                                                    */
/* -------------------------------------------------------------------------- */

/**
 * @brief List registered keyboard engine names.
 * @param[out] count Number of engines returned.
 * @return Array of NUL-terminated strings. Caller must release with
 *         `typio_free_string_array(list, count)`.
 */
char **typio_registry_list_keyboards(TypioRegistry *registry, size_t *count);

/** List registered voice engine names. */
char **typio_registry_list_voices(TypioRegistry *registry, size_t *count);

/**
 * @brief List keyboard engines in the order configured by `engine_order`.
 *
 * Unlisted engines fall back to registration order.
 */
char **typio_registry_list_ordered_keyboards(TypioRegistry *registry,
                                              size_t *count);

/* -------------------------------------------------------------------------- */
/* Engine info                                                                */
/* -------------------------------------------------------------------------- */

/**
 * @brief Return a fresh `TypioEngineInfo` for the named engine, or NULL.
 *
 * Caller must release with `typio_engine_info_free`.
 */
const TypioEngineInfo *typio_registry_get_engine_info(TypioRegistry *registry,
                                                       const char *name);

/**
 * @brief Free a `TypioEngineInfo` previously returned by
 *        `typio_registry_get_engine_info`.
 *
 * Frees all interior string pointers and the struct itself.
 */
void typio_engine_info_free(TypioEngineInfo *info);

/**
 * @brief Get the display name of a registered engine.
 * @return Freshly allocated NUL-terminated string, or NULL. Caller must
 *         release with `typio_free_string`.
 */
char *typio_registry_get_engine_display_name(TypioRegistry *registry,
                                              const char *name);

/**
 * @brief Get the icon identifier of a registered engine.
 *
 * Returns either a freedesktop icon name (e.g. "input-keyboard") or an
 * absolute file path to a PNG/SVG.
 *
 * @return Freshly allocated string, or NULL. Caller frees with
 *         `typio_free_string`.
 */
char *typio_registry_get_engine_icon(TypioRegistry *registry, const char *name);

/** Description, freshly allocated. Free with `typio_free_string`. */
char *typio_registry_get_engine_description(TypioRegistry *registry,
                                             const char *name);

/** Author, freshly allocated. Free with `typio_free_string`. */
char *typio_registry_get_engine_author(TypioRegistry *registry,
                                        const char *name);

/** Primary language code, freshly allocated. Free with `typio_free_string`. */
char *typio_registry_get_engine_language(TypioRegistry *registry,
                                          const char *name);

/* -------------------------------------------------------------------------- */
/* Activation / Switching                                                     */
/* -------------------------------------------------------------------------- */

TypioResult typio_registry_set_active_keyboard(TypioRegistry *registry,
                                                const char *name);
TypioResult typio_registry_set_active_voice(TypioRegistry *registry,
                                             const char *name);

/**
 * @brief Get the name of the currently active keyboard engine.
 * @return Freshly allocated string, or NULL. Caller frees with
 *         `typio_free_string`.
 */
char *typio_registry_get_active_keyboard(TypioRegistry *registry);

/** As above for the voice engine. */
char *typio_registry_get_active_voice(TypioRegistry *registry);

/** Return availability for the active keyboard engine. */
TypioEngineAvailability
typio_registry_get_active_keyboard_availability(TypioRegistry *registry);

/** Return availability for the active voice engine. */
TypioEngineAvailability
typio_registry_get_active_voice_availability(TypioRegistry *registry);

TypioResult typio_registry_next_keyboard(TypioRegistry *registry);
TypioResult typio_registry_prev_keyboard(TypioRegistry *registry);
TypioResult typio_registry_next_voice(TypioRegistry *registry);
TypioResult typio_registry_prev_voice(TypioRegistry *registry);

/* -------------------------------------------------------------------------- */
/* Language model (ADR-0018)                                                  */
/* -------------------------------------------------------------------------- */

/**
 * @brief Replace the declared language list of a registered engine.
 *
 * @param languages NULL-terminated array of BCP-47 tags, primary first. The
 *                  pseudo-tag "mul" declares support for every language.
 *                  Hosts call this right after registration with the
 *                  manifest's `languages` value.
 * @return TYPIO_OK, or TYPIO_ERROR_NOT_FOUND for an unknown engine.
 */
TypioResult typio_registry_set_engine_languages(TypioRegistry *registry,
                                                 const char *name,
                                                 const char *const *languages);

/**
 * @brief Declared language list of the named engine, primary first.
 * @param[out] count Number of tags returned.
 * @return Array of strings; release with `typio_free_string_array`.
 */
char **typio_registry_get_engine_languages(TypioRegistry *registry,
                                            const char *name,
                                            size_t *count);

/**
 * @brief Enabled language cycle.
 *
 * The `languages.enabled` config key (array or comma-separated string) when
 * set, otherwise every engine-declared language in registration order.
 *
 * @param[out] count Number of tags returned.
 * @return Array of strings; release with `typio_free_string_array`.
 */
char **typio_registry_list_languages(TypioRegistry *registry, size_t *count);

/**
 * @brief Active language tag.
 * @return Freshly allocated string, or NULL when no language was activated.
 *         Caller frees with `typio_free_string`.
 */
char *typio_registry_get_active_language(TypioRegistry *registry);

/**
 * @brief Activate a language: re-resolve and retarget every modality slot.
 *
 * Engine choice per modality follows `languages.<tag>.keyboard` /
 * `languages.<tag>.voice` (the string "none" forces an empty slot), then the
 * first registered engine declaring a matching language. A modality with no
 * engine is deactivated; for keyboards this yields raw passthrough
 * (layout-only languages).
 */
TypioResult typio_registry_set_active_language(TypioRegistry *registry,
                                                const char *tag);

/**
 * @brief Switch to the next/previous language in the enabled cycle.
 *
 * @return TYPIO_OK, or TYPIO_ERROR_NOT_FOUND when no languages are enabled
 *         or declared (hosts may fall back to engine cycling).
 */
TypioResult typio_registry_next_language(TypioRegistry *registry);
TypioResult typio_registry_prev_language(TypioRegistry *registry);

/**
 * @brief Activate the persisted last-used language, falling back to the
 *        first enabled language.
 *
 * Hosts call this once at startup after engine discovery.
 *
 * @return TYPIO_OK, or TYPIO_ERROR_NOT_FOUND when no languages are enabled
 *         or declared.
 */
TypioResult typio_registry_restore_language(TypioRegistry *registry);

/* -------------------------------------------------------------------------- */
/* Commit notification                                                        */
/* -------------------------------------------------------------------------- */

/**
 * @brief Notify the registry that the active keyboard engine committed text.
 *
 * Updates the keyboard recent-engine pair used for fast-toggle between two
 * keyboard engines.
 */
void typio_registry_notify_keyboard_commit(TypioRegistry *registry);

/** As above for voice engines. */
void typio_registry_notify_voice_commit(TypioRegistry *registry);

/* -------------------------------------------------------------------------- */
/* Engine command surface (ADR-0008)                                          */
/* -------------------------------------------------------------------------- */

/**
 * @brief Invoke a command on the active keyboard engine.
 *
 * Thin convenience over `typio_registry_invoke_command` that resolves the
 * active keyboard engine first.
 *
 * @param id The command identifier (e.g. "deploy").
 * @return TYPIO_OK on success, TYPIO_ERROR_NOT_FOUND if no keyboard is active,
 *         TYPIO_ERROR_ENGINE_NOT_AVAILABLE if the engine does not expose
 *         this command.
 */
TypioResult typio_registry_invoke_active_keyboard_command(TypioRegistry *registry,
                                                          const char *id);

/**
 * @brief Invoke a command on a named engine (any kind).
 *
 * @param engine_name The engine's stable name (e.g. "rime").
 * @param id          The command identifier (e.g. "deploy").
 * @return TYPIO_OK on success, TYPIO_ERROR_NOT_FOUND if no engine with that
 *         name is registered, TYPIO_ERROR_ENGINE_NOT_AVAILABLE if the engine
 *         does not expose this command.
 */
TypioResult typio_registry_invoke_command(TypioRegistry *registry,
                                          const char *engine_name,
                                          const char *id);

/**
 * @brief List the commands exposed by a named engine.
 *
 * Returns a freshly-allocated array of `TypioEngineCommand`. The array and
 * every interior string are owned by the caller and must be released with
 * `typio_engine_command_list_free`. Returns NULL with `*out_count = 0` when
 * the engine is unknown or exposes no commands.
 */
TypioEngineCommand *typio_registry_list_commands(TypioRegistry *registry,
                                                 const char *engine_name,
                                                 size_t *out_count);

/**
 * @brief Release a command array returned by `typio_registry_list_commands`.
 *
 * Frees the array and every interior string. No-op for NULL.
 */
void typio_engine_command_list_free(TypioEngineCommand *commands, size_t count);

/**
 * @brief Notify a named engine that one of its config keys changed.
 *
 * The host calls this after writing `engines.<name>.<key>` through the
 * unified config tree. No-op if the engine does not implement
 * `on_config_change` on its `TypioEngineBaseOps` vtable.
 *
 * @return TYPIO_OK on success, TYPIO_ERROR_NOT_FOUND if no engine with that
 *         name is registered.
 */
TypioResult typio_registry_notify_config_change(TypioRegistry *registry,
                                                const char *engine_name,
                                                const char *key,
                                                const char *value);

/**
 * @brief Activate the most recently used voice engine.
 *
 * Checks state-persistence first (engine-state.toml), then falls back to
 * the first registered voice engine.  The `voice.engine` config key takes
 * priority over this when called from `typio_instance_init()`.
 *
 * @return TYPIO_OK on success, TYPIO_ERROR_NOT_FOUND if no voice engine
 *         is registered.
 */
TypioResult typio_registry_activate_last_used_voice(TypioRegistry *registry);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_REGISTRY_H */
