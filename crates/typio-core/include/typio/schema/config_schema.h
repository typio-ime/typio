/**
 * @file config_schema.h
 * @brief Config schema registry — defaults and UI metadata for every
 *        configuration field, layered between a host-owned static base and
 *        engine-registered dynamic entries.
 *
 * The schema has two layers:
 *
 *   1. Static base — host-owned settings (display, notifications, shortcuts,
 *      voice, …) plus built-in `engines.compose.*` fields. Hardcoded in libtypio.
 *
 *   2. Dynamic layer — engine-owned `engines.<name>.*` fields registered at
 *      runtime via `typio_config_schema_register*`. Engine plugins call
 *      these from their loader/init path so the host never has to know which
 *      knobs an engine exposes.
 *
 * Lookup, default application, and field enumeration transparently see both
 * layers. Pointers returned by the read APIs are valid until the next
 * registration mutation; callers that need a longer-lived snapshot should
 * copy out.
 */

#ifndef TYPIO_CONFIG_SCHEMA_H
#define TYPIO_CONFIG_SCHEMA_H

#include "typio/abi/config.h"
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    TYPIO_FIELD_STRING = 0,
    TYPIO_FIELD_INT = 1,
    TYPIO_FIELD_BOOL = 2,
    TYPIO_FIELD_FLOAT = 3,
} TypioFieldType;

typedef struct TypioConfigField {
    const char *key;            /* canonical dotted key, e.g. "display.font_size" */
    TypioFieldType type;
    union {
        const char *s;
        int i;
        bool b;
        double f;
    } def;                      /* default value */

    /* UI metadata (ignored by server-side code) */
    const char *ui_label;
    const char *ui_section;     /* "display"|"notifications"|"keyboard"|"compose"|"rime"|"mozc"|"shortcuts"|"voice" */
    int ui_min, ui_max, ui_step;
    const char *const *ui_options; /* NULL-terminated string array for dropdowns, or NULL */
    const char *runtime_property;  /* matching D-Bus runtime property, or NULL */
} TypioConfigField;

/**
 * @brief Look up a schema field by canonical key.
 * @return Pointer into static table, or NULL if not found.
 */
const TypioConfigField *typio_config_schema_find(const char *key);

/**
 * @brief Get the runtime D-Bus property mirrored by a persisted config key.
 * @return Property name, or NULL if the key has no direct runtime mirror.
 */
const char *typio_config_schema_runtime_property(const char *key);

/**
 * @brief Apply default values from the schema for any key not already present.
 */
void typio_config_apply_defaults(TypioConfig *config);

/**
 * @brief Get the full schema table (static base + dynamic).
 * @param[out] count Number of entries.
 * @return Pointer to the combined array, valid until the next
 *         `typio_config_schema_register*` or `_unregister` call.
 */
const TypioConfigField *typio_config_schema_fields(size_t *count);

/**
 * @brief Return pointers to every schema field whose key starts with @p prefix.
 *
 * Useful for enumerating an engine's properties: pass `"engines.rime."` to
 * get every field rime registered.
 *
 * @param prefix     Key prefix to match (literal compare, no glob).
 * @param[out] out_count Number of matched fields written here.
 * @return Newly-allocated array of *borrowed* `TypioConfigField *` (the
 *         fields themselves remain owned by the registry and are valid
 *         until the next `typio_config_schema_register*`/`_unregister`
 *         call). Caller releases the outer array with
 *         `typio_config_schema_fields_with_prefix_free`. NULL with
 *         `*out_count = 0` on no match or invalid args.
 */
const TypioConfigField **typio_config_schema_fields_with_prefix(
    const char *prefix, size_t *out_count);

/**
 * @brief Release an array returned by `typio_config_schema_fields_with_prefix`.
 *
 * Does NOT free the pointed-to fields. No-op for NULL.
 */
void typio_config_schema_fields_with_prefix_free(
    const TypioConfigField **fields, size_t count);

/* -------------------------------------------------------------------------- */
/* Runtime registration (engine-owned fields)                                 */
/* -------------------------------------------------------------------------- */

/**
 * @brief Register a single engine-owned schema field.
 *
 * Every string reachable from @p field (key, ui_label, ui_section, the entries
 * of `ui_options`, runtime_property, and the string default if applicable) is
 * deep-copied; the caller may free or stack-drop its storage after the call
 * returns.
 *
 * @return TYPIO_OK on success, TYPIO_ERROR_INVALID_ARGUMENT if @p field or its
 *         key is NULL, TYPIO_ERROR_ALREADY_EXISTS if the key collides with an
 *         existing static or dynamic entry.
 */
TypioResult typio_config_schema_register(const TypioConfigField *field);

/**
 * @brief Register an array of schema fields.
 *
 * Equivalent to calling `typio_config_schema_register` for each entry; stops
 * on the first error and returns it. Fields that succeeded before the error
 * remain registered.
 *
 * Engine plugins typically declare a static `TypioConfigField[]` table and
 * call this once from their plugin entry point (or `init`).
 */
TypioResult typio_config_schema_register_many(const TypioConfigField *fields,
                                              size_t count);

/**
 * @brief Remove a previously-registered dynamic field by key.
 *
 * Static fields cannot be unregistered. Engines should unregister their
 * fields when their plugin is unloaded.
 *
 * @return TYPIO_OK on success, TYPIO_ERROR_NOT_FOUND if no dynamic field with
 *         that key is registered.
 */
TypioResult typio_config_schema_unregister(const char *key);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_CONFIG_SCHEMA_H */
