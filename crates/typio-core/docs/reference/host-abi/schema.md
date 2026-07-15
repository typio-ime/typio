# Config Schema Reference

`TypioConfigField` is the per-key descriptor — type, default, and UI metadata
— for every configuration key Typio knows about.

Header: `typio/schema/config_schema.h`. Field values are stored in
[`TypioConfig`](config.md). Result codes are in [types.md](types.md#typioresult).

## Layers

| Layer | Owner | Source | Mutable at runtime? |
|-------|-------|--------|---------------------|
| Static base | libtypio | Compiled into `libtypio.so` (`src/config_schema.rs`) | No |
| Dynamic | Engine worker (or any caller) | Published in EngineHello and registered via `typio_config_schema_register*` | Yes |

Lookup, default application, and field enumeration see both layers
transparently. Engine workers own every `engines.<name>.*` key — including
the built-in-feeling `engines.compose.*`, which ships as the
`typio-engine-compose` worker. The static base only covers framework-level
policy; frontend presentation keys live in frontend-owned config files.

## `TypioFieldType`

```c
typedef enum {
    TYPIO_FIELD_STRING = 0,
    TYPIO_FIELD_INT    = 1,
    TYPIO_FIELD_BOOL   = 2,
    TYPIO_FIELD_FLOAT  = 3,
} TypioFieldType;
```

## `TypioConfigField`

```c
typedef struct TypioConfigField {
    const char    *key;
    TypioFieldType type;
    union { const char *s; int i; bool b; double f; } def;

    const char    *ui_label;
    const char    *ui_section;
    int            ui_min, ui_max, ui_step;
    const char *const *ui_options;
    const char    *runtime_property;
} TypioConfigField;
```

| Field | Purpose |
|-------|---------|
| `key` | Canonical dotted path, e.g. `display.font_size` or `engines.rime.schema` |
| `type` | Variant of `def` and the typed getter used for this key |
| `def` | Default value applied by `typio_config_apply_defaults`. Empty string is treated as "no default" (key stays unset). |
| `ui_label` | Display label for control surfaces. `NULL` means the key is internal (no UI). |
| `ui_section` | Logical grouping, e.g. `display`, `shortcuts`, `<engine-name>` |
| `ui_min`/`ui_max`/`ui_step` | Range hints for numeric fields. Ignored when both `min` and `max` are `0`. |
| `ui_options` | NULL-terminated string array for dropdowns. NULL for free-form. |
| `runtime_property` | Optional host-defined runtime state key, or NULL. See [Config & Runtime Ownership](../../explanation/config-runtime-ownership.md). |

## Lookup

```c
const TypioConfigField *typio_config_schema_find(const char *key);
const TypioConfigField *typio_config_schema_fields(size_t *count);
const char             *typio_config_schema_runtime_property(const char *key);
```

| Function | Returns | Notes |
|----------|---------|-------|
| `typio_config_schema_find` | Pointer into the combined table, or NULL | Searches static base **and** dynamic registrations |
| `typio_config_schema_fields` | Pointer to the flat combined table | `*count` is written if non-NULL |
| `typio_config_schema_runtime_property` | Host-defined runtime state key, or NULL | NULL when the field has no `runtime_property` |

Pointer-stability contract: pointers returned by the read APIs (and the
strings they reference) are valid until the next `register*` / `unregister`
call. Callers needing a long-lived snapshot should copy out. In practice
the host installs each engine schema during the executable's discovery probe,
before any UI consumer queries it.

## Defaults

```c
void typio_config_apply_defaults(TypioConfig *config);
```

| Behaviour | Detail |
|-----------|--------|
| Missing key | Set to the field's `def` value |
| Existing key | Left untouched |
| String default of `""` | Skipped — the key remains absent so user-blank can be distinguished from "never set" |
| Dynamic fields | Applied alongside the static base in a single pass |

Apply this after every `load_file`, `load_string`, or `ReloadConfig()` to
materialise the complete configuration the runtime expects.

## Registration

```c
TypioResult typio_config_schema_register     (const TypioConfigField *field);
TypioResult typio_config_schema_register_many(const TypioConfigField *fields,
                                              size_t count);
TypioResult typio_config_schema_unregister   (const char *key);
```

| Function | Returns |
|----------|---------|
| `typio_config_schema_register` | `TYPIO_OK`, `TYPIO_ERROR_INVALID_ARGUMENT` on NULL field or empty key, `TYPIO_ERROR_ALREADY_EXISTS` if the key collides with an existing static or dynamic entry |
| `typio_config_schema_register_many` | `TYPIO_OK`, or the first error encountered. Fields that succeeded before the error remain registered. |
| `typio_config_schema_unregister` | `TYPIO_OK`, `TYPIO_ERROR_NOT_FOUND` if no dynamic field with that key. Static fields cannot be unregistered. |

Ownership: every string reachable from a passed `TypioConfigField` (`key`,
`ui_label`, `ui_section`, each entry of `ui_options`, `runtime_property`,
and the string default) is deep-copied during registration. The caller may
free or stack-drop its storage immediately after the call returns.

## Recommended Engine Worker Pattern

Declare a static field table and expose it through
[`typio_engine_get_config_schema`](../engine/entry.md#optional-typio_engine_get_config_schema):

```c
static const char *const RIME_FULL_CHECK_OPTIONS[] = { "auto", "always", "never", NULL };

static const TypioConfigField RIME_SCHEMA[] = {
    {
        .key = "engines.rime.shared_data_dir",
        .type = TYPIO_FIELD_STRING,
        .def = { .s = "" },
        .ui_label = "Shared data dir",
        .ui_section = "rime",
    },
    {
        .key = "engines.rime.user_data_dir",
        .type = TYPIO_FIELD_STRING,
        .def = { .s = "" },
        .ui_label = "User data dir",
        .ui_section = "rime",
    },
    {
        .key = "engines.rime.full_check",
        .type = TYPIO_FIELD_STRING,
        .def = { .s = "auto" },
        .ui_label = "Full deploy check",
        .ui_section = "rime",
        .ui_options = RIME_FULL_CHECK_OPTIONS,
    },
};

const TypioConfigField *typio_engine_get_config_schema(size_t *out_count) {
    *out_count = sizeof(RIME_SCHEMA) / sizeof(RIME_SCHEMA[0]);
    return RIME_SCHEMA;
}
```

The canonical worker harness registers this table in its local schema before
creating `TypioInstance`, and serialises it into EngineHello for the host. The
host validates the `engines.<name>.` namespace and replaces that engine's
previous dynamic schema atomically.

## See also

- [Config](config.md) — `TypioConfig` storage and accessors that look up values by key
- [Engine entry points](../engine/entry.md) — engine-side exports including the optional schema function
- [Configuration Reference](../configuration.md) — user-facing list of host-owned keys
- [Configuration System](../../explanation/configuration-system.md) — design rationale and lifecycle
