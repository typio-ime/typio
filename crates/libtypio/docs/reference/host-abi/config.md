# Config Reference

`TypioConfig` is libtypio's TOML-compatible flat-key configuration store.

Header: `typio/abi/config.h`. Result codes are in [types.md](types.md#typioresult).

## Key path syntax

Keys are TOML-style dotted paths over the underlying tree:

| Example | Refers to |
|---------|-----------|
| `"engine_order"` | Top-level scalar |
| `"engines.rime.schema"` | Nested scalar under table `engines.rime` |
| `"shortcuts.switch_keyboard_engine"` | Nested scalar under table `shortcuts` |

Dots are path separators; arrays are accessed through the dedicated array
getters (see [Array access](#array-access)).

## `TypioConfigType`

```c
typedef enum {
    TYPIO_CONFIG_STRING = 0,
    TYPIO_CONFIG_INT    = 1,
    TYPIO_CONFIG_BOOL   = 2,
    TYPIO_CONFIG_FLOAT  = 3,
    TYPIO_CONFIG_ARRAY  = 4,
    TYPIO_CONFIG_OBJECT = 5,
} TypioConfigType;
```

Type tag for values stored in a config tree. Exposed for tooling and schema
introspection; typed getters/setters below abstract over it.

## Lifecycle

```c
TypioConfig *typio_config_new(void);
TypioConfig *typio_config_load_file(const char *path);
TypioConfig *typio_config_load_string(const char *content);
void         typio_config_free(TypioConfig *config);
```

| Function | Returns | Notes |
|----------|---------|-------|
| `typio_config_new` | Empty config | Never NULL on success |
| `typio_config_load_file` | Loaded config, or NULL on read/parse failure | Path must be UTF-8 |
| `typio_config_load_string` | Loaded config, or NULL on parse failure | TOML 1.0 syntax |
| `typio_config_free` | — | Safe to call with NULL |

## Persistence

```c
TypioResult typio_config_save_file(const TypioConfig *config, const char *path);
char       *typio_config_to_string(const TypioConfig *config);
```

| Function | Returns | Ownership |
|----------|---------|-----------|
| `typio_config_save_file` | `TYPIO_OK`, `TYPIO_ERROR_INVALID_ARGUMENT`, or `TYPIO_ERROR` on I/O failure | Creates parent directories if needed |
| `typio_config_to_string` | Serialised TOML, or NULL on failure | Caller frees with **`typio_free_string`** (not `free()` — see [memory ownership](../../dev/contract-layers.md#memory-ownership)) |

## Typed getters

```c
const char *typio_config_get_string(const TypioConfig *config, const char *key,
                                    const char *default_val);
int         typio_config_get_int   (const TypioConfig *config, const char *key,
                                    int default_val);
bool        typio_config_get_bool  (const TypioConfig *config, const char *key,
                                    bool default_val);
double      typio_config_get_float (const TypioConfig *config, const char *key,
                                    double default_val);
```

| Behaviour | Detail |
|-----------|--------|
| Missing key | Returns `default_val`. No error signal. |
| Key exists, wrong type | Returns `default_val`. No type coercion. |
| `config == NULL` or `key == NULL` | Returns `default_val`. |
| String lifetime (`get_string`) | Pointer borrowed from the config; valid until the next mutation (`set_*` / `remove` / `merge`) or `typio_config_free`. Copy if retaining across mutations. |

## Typed setters

```c
TypioResult typio_config_set_string      (TypioConfig *config, const char *key,
                                          const char *value);
TypioResult typio_config_set_int         (TypioConfig *config, const char *key,
                                          int value);
TypioResult typio_config_set_bool        (TypioConfig *config, const char *key,
                                          bool value);
TypioResult typio_config_set_float       (TypioConfig *config, const char *key,
                                          double value);
TypioResult typio_config_set_string_array(TypioConfig *config, const char *key,
                                          const char *const *values, size_t count);
```

| Detail | |
|--------|--|
| Missing intermediate tables | Created automatically when setting a nested key |
| Existing value of different type | Overwritten |
| Storage of `value` (`set_string`) | Copied into the config; caller's pointer not retained |
| Storage of `values` (`set_string_array`) | Each element copied; caller may free the array after the call |
| Errors | `TYPIO_OK`, `TYPIO_ERROR_INVALID_ARGUMENT` on NULL, `TYPIO_ERROR_OUT_OF_MEMORY` |

## Array access

Read-only typed views over array values.

```c
size_t       typio_config_get_array_size  (const TypioConfig *config, const char *key);
const char  *typio_config_get_array_string(const TypioConfig *config, const char *key,
                                           size_t index);
int          typio_config_get_array_int   (const TypioConfig *config, const char *key,
                                           size_t index);
```

| Function | Returns on missing/out-of-range |
|----------|---------------------------------|
| `get_array_size` | `0` (key missing or not an array) |
| `get_array_string` | NULL |
| `get_array_int` | `0` |

Returned strings are borrowed with the same lifetime as `get_string`.

## Nested sections

```c
TypioConfig *typio_config_get_section(const TypioConfig *config,
                                      const char *section);
TypioResult  typio_config_set_section(TypioConfig *config, const char *section,
                                      TypioConfig *sub_config);
```

| Function | Behaviour |
|----------|-----------|
| `get_section` | Returns a newly allocated copy of the subtree, or NULL if the section is missing. Caller frees with `typio_config_free`. |
| `set_section` | Replaces (or inserts) the subtree at `section` with a copy of `sub_config`. The caller still owns `sub_config` and must free it. |

## Key enumeration

```c
size_t typio_config_key_count(const TypioConfig *config);
char  *typio_config_key_at   (const TypioConfig *config, size_t index);
bool   typio_config_has_key  (const TypioConfig *config, const char *key);
```

| Function | Notes |
|----------|-------|
| `key_count` | Number of **top-level** keys; does not recurse into nested tables |
| `key_at` | Newly allocated key name; caller frees with `typio_free_string`. Returns NULL if `index` is out of range. |
| `has_key` | Matches the full dotted path, including nested keys |

## Removal and merging

```c
TypioResult typio_config_remove(TypioConfig *config, const char *key);
TypioResult typio_config_merge (TypioConfig *dest, const TypioConfig *src);
```

| Function | Behaviour |
|----------|-----------|
| `remove` | Deletes `key` if present. `TYPIO_OK` whether or not the key existed; `TYPIO_ERROR_INVALID_ARGUMENT` on NULL. |
| `merge` | Deep merge `src` into `dest`. Scalars from `src` overwrite scalars in `dest`; tables are merged recursively; arrays are replaced wholesale. |

## See also

- [Shared types](types.md) — `TypioResult`, opaque handles
- [Instance](instance.md) — `typio_instance_get_config`, `typio_instance_reload_config`, `typio_instance_save_config`
- [Schema](schema.md) — `TypioConfigField`, defaults, engine-registered fields
- [Configuration Reference](../configuration.md) — every `core.toml` key, its type, and default
- [Contract layers ▸ Memory ownership](../../dev/contract-layers.md#memory-ownership) — the `typio_free_*` family
