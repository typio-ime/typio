/**
 * @file abi/instance.h
 * @brief Engine-facing TypioInstance operations (part of the plugin ABI).
 *
 * Functions in this header are callable by engine plugins.  They form
 * the read/observe/notify surface that engines need to participate in
 * the runtime without being coupled to host or core internals.
 *
 * Host-only lifecycle, callback registration, voice session, and
 * identity persistence live in typio/runtime/instance.h and must NOT
 * be called by engines.
 */

#ifndef TYPIO_ABI_INSTANCE_H
#define TYPIO_ABI_INSTANCE_H

#include "typio/abi/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ── Focus / context ──────────────────────────────────────────────────── */

/** @brief The currently focused input context, or NULL if none. */
TypioInputContext *typio_instance_get_focused_context(TypioInstance *instance);

/* ── Directory accessors ──────────────────────────────────────────────── */

const char *typio_instance_get_config_dir(TypioInstance *instance);
const char *typio_instance_get_data_dir(TypioInstance *instance);
const char *typio_instance_get_state_dir(TypioInstance *instance);

/**
 * @brief Engine-scoped data directory.
 *
 * Returns `<data_dir>/<engine_name>/`, creating it if necessary.
 * The returned pointer is borrowed from the instance and remains valid
 * until the instance is freed.
 *
 * Engines should use this instead of manually constructing paths or
 * requiring the user to configure `user_data_dir` in core.toml.
 * Internal directory layout within this path is entirely engine-defined.
 *
 * @param instance     A live TypioInstance.
 * @param engine_name  Engine identifier (e.g. "rime").
 * @return Borrowed path string, or NULL on invalid arguments.
 */
const char *typio_instance_get_engine_data_dir(TypioInstance *instance,
                                                const char *engine_name);

/**
 * @brief Engine-scoped state directory.
 *
 * Same contract as typio_instance_get_engine_data_dir but rooted under
 * the instance state directory (`<state_dir>/<engine_name>/`).
 */
const char *typio_instance_get_engine_state_dir(TypioInstance *instance,
                                                 const char *engine_name);

/* ── Configuration ────────────────────────────────────────────────────── */

/** @brief The live root configuration object owned by the instance. */
TypioConfig *typio_instance_get_config(TypioInstance *instance);

/** @brief A newly-allocated copy of the engine's config section.
 *  @param engine_name Engine name (e.g. "rime"). Caller frees with
 *                     typio_config_free(). */
TypioConfig *typio_instance_get_engine_config(TypioInstance *instance,
                                               const char *engine_name);

TypioResult typio_instance_reload_config(TypioInstance *instance);
TypioResult typio_instance_save_config(TypioInstance *instance);

/**
 * @brief Write an engine-owned config key, persist, and notify the engine.
 *
 * Engines call this from their surface-command handlers (e.g. "setup") to
 * update their own configuration keys.  The key is scoped under
 * `engines.<engine_name>.<key>` — the caller provides only the short key
 * name (e.g. "model"), not the full dotted path.
 *
 * The function validates that the full key exists in the config schema (the
 * engine must have registered it via typio_config_schema_register* during
 * plugin load), writes the value, saves the config file, and fires
 * on_config_change on the target engine.
 *
 * @return TypioOk on success.
 *         TypioErrorNotFound if the key is not in the schema.
 *         TypioErrorInvalidArgument if instance or engine_name is NULL.
 */
TypioResult typio_instance_set_engine_config_key(TypioInstance *instance,
                                                 const char *engine_name,
                                                 const char *key,
                                                 const char *value);

/* ── Status icon (single-string surface) ──────────────────────────────── */

void typio_instance_notify_status_icon(TypioInstance *instance,
                                        const char *icon_name);
void typio_instance_clear_status_icon(TypioInstance *instance);
const char *typio_instance_get_last_status_icon(TypioInstance *instance);

/* ── Engine mode notification ─────────────────────────────────────────── */

/** @brief Engines call this whenever their active mode changes.
 *
 *  Stores a copy of @p mode and fires the mode-changed callback (and
 *  the status-icon-changed callback derived from icon_name).
 *  No-op if @p mode.id equals the previously notified mode id. */
void typio_instance_notify_keyboard_mode(TypioInstance *instance,
                                  const TypioKeyboardEngineMode *mode);
void typio_instance_clear_keyboard_mode(TypioInstance *instance);
const TypioKeyboardEngineMode *typio_instance_get_last_keyboard_mode(TypioInstance *instance);

/* ── Engine availability notification (ADR-0014) ──────────────────────── */

/** @brief Engines call this whenever their availability changes.
 *
 *  Caches @p state (and @p reason, which may be NULL) and fires the
 *  availability-changed callback. No-op if @p state equals the previously
 *  notified state. The host gates input routing on the cached value. */
void typio_instance_notify_engine_availability(TypioInstance *instance,
                                               TypioEngineAvailability state,
                                               const char *reason);
TypioEngineAvailability typio_instance_get_engine_availability(TypioInstance *instance);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_ABI_INSTANCE_H */
