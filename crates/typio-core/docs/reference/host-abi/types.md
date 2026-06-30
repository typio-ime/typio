# Shared Types Reference

ABI types shared across the host surface. Individual pages link here rather
than redefining these in place.

Header: `typio/abi/types.h`.

## `TypioResult`

```c
typedef enum {
    TYPIO_OK                          =  0,
    TYPIO_ERROR                       = -1,
    TYPIO_ERROR_INVALID_ARGUMENT      = -2,
    TYPIO_ERROR_OUT_OF_MEMORY         = -3,
    TYPIO_ERROR_NOT_FOUND             = -4,
    TYPIO_ERROR_ALREADY_EXISTS        = -5,
    TYPIO_ERROR_NOT_INITIALIZED       = -6,
    TYPIO_ERROR_ENGINE_LOAD_FAILED    = -7,
    TYPIO_ERROR_ENGINE_NOT_AVAILABLE  = -8,
} TypioResult;
```

| Value | Meaning |
|-------|---------|
| `TYPIO_OK` | Success |
| `TYPIO_ERROR` | Generic failure; check logs for details |
| `TYPIO_ERROR_INVALID_ARGUMENT` | NULL pointer, bad key syntax, or out-of-range value |
| `TYPIO_ERROR_OUT_OF_MEMORY` | Allocation failed |
| `TYPIO_ERROR_NOT_FOUND` | Engine name, config key, or resource does not exist |
| `TYPIO_ERROR_ALREADY_EXISTS` | Engine name collision at registration |
| `TYPIO_ERROR_NOT_INITIALIZED` | `typio_instance_init` not yet called |
| `TYPIO_ERROR_ENGINE_LOAD_FAILED` | Plugin discovered but failed to load or initialise |
| `TYPIO_ERROR_ENGINE_NOT_AVAILABLE` | Activation requested but no engine of that name is registered |

Any negative value indicates failure. `TYPIO_OK == 0` is the only success
sentinel; do not pattern-match other zero values.

## `TypioEngineAvailability`

```c
typedef enum {
    TYPIO_ENGINE_UNINITIALIZED = 0,
    TYPIO_ENGINE_PREPARING     = 1,
    TYPIO_ENGINE_READY         = 2,
    TYPIO_ENGINE_FAILED        = 3,
} TypioEngineAvailability;
```

| Value | Meaning | Host routing |
|-------|---------|--------------|
| `TYPIO_ENGINE_UNINITIALIZED` | Engine exists but `init` has not completed | Do not route input |
| `TYPIO_ENGINE_PREPARING` | Engine is doing asynchronous warm-up | Do not route input |
| `TYPIO_ENGINE_READY` | Engine can process input | Route input |
| `TYPIO_ENGINE_FAILED` | Engine warm-up failed | Do not route input |

## Opaque handle types

These are forward-declared in `typio/abi/types.h` and have no public layout.
Hosts hold pointers; allocation and disposal go through the named
constructor/destructor pairs.

| Handle | Created by | Destroyed by | Owned by |
|--------|------------|--------------|----------|
| `TypioInstance *` | `typio_instance_new` / `_new_with_config` | `typio_instance_free` | Host |
| `TypioRegistry *` | Lives inside `TypioInstance`; accessed via `typio_instance_get_registry` | Auto, with the instance | Instance |
| `TypioInputContext *` | `typio_instance_create_context` or `typio_input_context_new` | `typio_instance_destroy_context` or `typio_input_context_free` | Host |
| `TypioConfig *` | `typio_config_new` / `_load_file` / `_load_string` / `typio_instance_get_engine_config` | `typio_config_free` | Caller (except `typio_instance_get_config`, which returns the instance's live root — do **not** free) |
| `TypioVoiceSession *` | Host constructs and registers via `typio_instance_set_voice_session` | Host | Host |

## Callback typedefs

Header: `typio/abi/types.h`. Each callback is invoked on libtypio's internal
runtime thread (the host's event loop, in practice); callbacks MUST NOT block
or re-enter libtypio with the same lock held.

```c
typedef void (*TypioCommitCallback)(
    TypioInputContext *ctx, const char *text, void *user_data);

typedef void (*TypioCompositionCallback)(
    TypioInputContext *ctx, const TypioComposition *composition, void *user_data);

typedef void (*TypioEngineChangedCallback)(
    TypioInstance *instance, const TypioEngineInfo *engine, void *user_data);

typedef void (*TypioVoiceEngineChangedCallback)(
    TypioInstance *instance, const TypioEngineInfo *engine, void *user_data);

typedef void (*TypioStatusIconChangedCallback)(
    TypioInstance *instance, const char *icon_name, void *user_data);

typedef void (*TypioKeyboardModeChangedCallback)(
    TypioInstance *instance, const TypioKeyboardEngineMode *mode, void *user_data);

typedef void (*TypioEngineAvailabilityChangedCallback)(
    TypioInstance *instance,
    TypioEngineAvailability state,
    const char *reason,
    void *user_data);
```

| Callback | Fires when | Pointer lifetime |
|----------|------------|------------------|
| `TypioCommitCallback` | Engine commits text via `typio_input_context_commit` | `text` valid only for the call; copy if retained |
| `TypioCompositionCallback` | Engine emits a new composition snapshot | `composition` and all nested pointers valid only for the call |
| `TypioEngineChangedCallback` | Active keyboard engine changed | `engine` may be NULL (no active engine) |
| `TypioVoiceEngineChangedCallback` | Active voice engine changed | Same as above |
| `TypioStatusIconChangedCallback` | Engine calls `typio_instance_notify_status_icon` | `icon_name` valid only for the call; may be NULL on clear |
| `TypioKeyboardModeChangedCallback` | Engine calls `typio_instance_notify_keyboard_mode` | `mode` valid only for the call; may be NULL on clear |
| `TypioEngineAvailabilityChangedCallback` | Engine calls `typio_instance_notify_engine_availability` or active engine changes | `reason` valid only for the call; may be NULL |

## See also

- [Event](event.md) — `TypioKeyEvent`, `TypioEventType`, `TypioModifier`
- [Input Context](input-context.md) — `TypioPreedit`, `TypioComposition`, `TypioKeyProcessResult`, `TypioContextCapability`
- [Engine ▸ Types](../engine/types.md) — `TypioEngineInfo`, `TypioEngine`, `TypioKeyboardEngineMode`
- [Contract layers ▸ Memory ownership](../../dev/contract-layers.md#memory-ownership) — the `typio_free_*` family
