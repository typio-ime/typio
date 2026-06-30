# Registry API Reference

`TypioRegistry` is the sole engine-management surface exposed to C callers
([ADR-0005](../../adr/0005-internal-engine-backend-abstraction.md)).

The registry owns two slots (one active keyboard, one active voice) and a
list of registered engines. Hosts construct it indirectly through
`typio_instance_init`; the registry is accessible via
`typio_instance_get_registry`.

## Lifecycle

```c
TypioRegistry *typio_registry_new(TypioInstance *instance);
void typio_registry_free(TypioRegistry *registry);
```

Hosts that drive their own lifecycle (rare — usually the instance handles
this) can construct a registry directly.

## Engine Process Registration

```c
TypioResult typio_registry_register_engine_process(
    TypioRegistry *registry,
    const TypioEngineInfo *info,
    const char *const *argv);

TypioResult typio_registry_unload(TypioRegistry *registry, const char *name);
```

| Parameter | Value |
|-----------|-------|
| `registry` | Registry returned by `typio_instance_get_registry` or `typio_registry_new` |
| `info` | Engine metadata snapshot copied by libtypio |
| `argv` | NULL-terminated engine argv; `argv[0]` is the executable |

| Result | Meaning |
|--------|---------|
| `TYPIO_OK` | Engine registered |
| `TYPIO_ERROR_INVALID_ARGUMENT` | NULL registry, metadata, name, or argv |
| `TYPIO_ERROR_ALREADY_EXISTS` | Engine name already registered |

## Listing

```c
char **typio_registry_list_keyboards(TypioRegistry *registry, size_t *count);
char **typio_registry_list_voices(TypioRegistry *registry, size_t *count);
char **typio_registry_list_ordered_keyboards(TypioRegistry *registry, size_t *count);
```

Each call returns a fresh array. Release with
`typio_free_string_array(list, count)`.

## Engine info

```c
const TypioEngineInfo *typio_registry_get_engine_info(TypioRegistry *registry,
                                                       const char *name);
void typio_engine_info_free(TypioEngineInfo *info);

char *typio_registry_get_engine_display_name(TypioRegistry *registry, const char *name);
char *typio_registry_get_engine_icon(TypioRegistry *registry, const char *name);
char *typio_registry_get_engine_description(TypioRegistry *registry, const char *name);
char *typio_registry_get_engine_author(TypioRegistry *registry, const char *name);
char *typio_registry_get_engine_language(TypioRegistry *registry, const char *name);
```

The full `TypioEngineInfo` snapshot is released with
`typio_engine_info_free`; the individual `char *` getters are released
with `typio_free_string`.

## Activation

```c
TypioResult typio_registry_set_active_keyboard(TypioRegistry *registry, const char *name);
TypioResult typio_registry_set_active_voice(TypioRegistry *registry, const char *name);

char *typio_registry_get_active_keyboard(TypioRegistry *registry);
char *typio_registry_get_active_voice(TypioRegistry *registry);

TypioEngineAvailability
typio_registry_get_active_keyboard_availability(TypioRegistry *registry);
TypioEngineAvailability
typio_registry_get_active_voice_availability(TypioRegistry *registry);
```

Activation fires the engine-changed callback registered on the parent
`TypioInstance`. The getters return a freshly allocated string (or NULL
when no engine is active); release with `typio_free_string`.

| Function | Return |
|----------|--------|
| `typio_registry_get_active_keyboard_availability` | Active keyboard engine availability; `TYPIO_ENGINE_FAILED` for NULL registry or no active keyboard |
| `typio_registry_get_active_voice_availability` | Active voice engine availability; `TYPIO_ENGINE_FAILED` for NULL registry or no active voice |

## Switching

```c
TypioResult typio_registry_next_keyboard(TypioRegistry *registry);
TypioResult typio_registry_prev_keyboard(TypioRegistry *registry);
TypioResult typio_registry_next_voice(TypioRegistry *registry);
TypioResult typio_registry_prev_voice(TypioRegistry *registry);
```

`next` / `prev` operate over the ordered keyboard / voice lists.

## Commit notification

```c
void typio_registry_notify_keyboard_commit(TypioRegistry *registry);
void typio_registry_notify_voice_commit(TypioRegistry *registry);
```

The active engine calls these after committing text; the registry uses the
signal to maintain the category-specific recent-engine pair used for
fast-toggle switching.
