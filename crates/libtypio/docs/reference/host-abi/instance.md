# Instance Reference

`TypioInstance` is the top-level libtypio object: it owns the engine registry,
input contexts, the live config, host callbacks, the voice session, and
per-application identity persistence.

Headers: `typio/runtime/instance.h` (host-only) plus `typio/abi/instance.h`
(engine-facing portion). Shared types in [types.md](types.md).

## `TypioInstanceConfig`

```c
typedef struct TypioInstanceConfig {
    const char            *config_dir;
    const char            *data_dir;
    const char            *state_dir;
    const char *const     *engine_dirs;
    TypioPluginLoaderFunc  plugin_loader;
    void                  *plugin_loader_user_data;
} TypioInstanceConfig;
```

| Field | Nullable | Meaning |
|-------|----------|---------|
| `config_dir` | No | Directory containing `core.toml` |
| `data_dir` | No | Directory for engine data and schemas |
| `state_dir` | No | Directory for runtime state (user dictionary, learning) |
| `engine_dirs` | Yes | NULL-terminated list of engine directories. NULL skips engine discovery entirely. |
| `plugin_loader` | Yes | Host callback invoked once per `engine_dirs` entry. NULL → no engines loaded. |
| `plugin_loader_user_data` | — | Opaque pointer forwarded to `plugin_loader` |

## `TypioPluginLoaderFunc`

```c
typedef int (*TypioPluginLoaderFunc)(TypioRegistry *registry,
                                     const char *dir,
                                     void *user_data);
```

| Aspect | Detail |
|--------|--------|
| Called | Once per directory in `TypioInstanceConfig::engine_dirs`, during `typio_instance_init`, after the registry is created and before last-used engine state is restored |
| Responsibility | Enumerate engine manifests in `dir` and register each accepted engine process via `typio_registry_register_engine_process` |
| Return | Number of engines successfully registered. Used for logging only; does not affect init success. |

## Lifecycle

```c
TypioInstance *typio_instance_new(void);
TypioInstance *typio_instance_new_with_config(const TypioInstanceConfig *config);
void           typio_instance_free(TypioInstance *instance);

TypioResult    typio_instance_init    (TypioInstance *instance);
void           typio_instance_shutdown(TypioInstance *instance);
```

| Function | Notes |
|----------|-------|
| `typio_instance_new` | Equivalent to `_new_with_config(NULL)` with default directories |
| `typio_instance_new_with_config` | Copies `config`; does **not** call `init`. Returns NULL on allocation failure. |
| `typio_instance_free` | Safe with NULL. Calls `shutdown` first if needed. |
| `typio_instance_init` | Idempotent. `TYPIO_OK` or `TYPIO_ERROR_*`. Logging MUST be configured first (`typio_logger_set_callback`). |
| `typio_instance_shutdown` | Idempotent. Reverses `init`; does **not** free the instance. |

## Registry access

```c
TypioRegistry *typio_instance_get_registry(TypioInstance *instance);
```

Returns the instance-owned registry. Do **not** free; it lives until
`typio_instance_free`.

## Input contexts

```c
TypioInputContext *typio_instance_create_context     (TypioInstance *instance);
void               typio_instance_destroy_context    (TypioInstance *instance,
                                                      TypioInputContext *ctx);
void               typio_instance_set_focused_context(TypioInstance *instance,
                                                      TypioInputContext *ctx);
TypioInputContext *typio_instance_get_focused_context(TypioInstance *instance);
```

| Function | Notes |
|----------|-------|
| `create_context` | Allocates a new context bound to this instance |
| `destroy_context` | Releases the context. Clears focus if `ctx` was focused. |
| `set_focused_context` | Replaces the currently focused context. NULL clears focus. |
| `get_focused_context` | Returns the focused context, or NULL. **Available to engines** (declared in `typio/abi/instance.h`). |

## Directory accessors (engine-facing)

```c
const char *typio_instance_get_config_dir(TypioInstance *instance);
const char *typio_instance_get_data_dir  (TypioInstance *instance);
const char *typio_instance_get_state_dir (TypioInstance *instance);
```

Pointers borrowed from the instance; valid for its lifetime.

## Configuration

### Live root config (engine-facing)

```c
TypioConfig *typio_instance_get_config       (TypioInstance *instance);
TypioConfig *typio_instance_get_engine_config(TypioInstance *instance,
                                              const char *engine_name);

TypioResult  typio_instance_reload_config(TypioInstance *instance);
TypioResult  typio_instance_save_config  (TypioInstance *instance);
```

| Function | Return ownership |
|----------|------------------|
| `get_config` | Borrowed — live root, owned by the instance. **Do not free.** Pointer remains valid until `typio_instance_free`. |
| `get_engine_config` | Newly allocated copy of the `engines.<name>` subtree. Caller frees with `typio_config_free`. |
| `reload_config` | Re-reads `config_dir/core.toml`, replaces the live root, fires per-engine `reload_config` ops |
| `save_config` | Serialises the live root to `config_dir/core.toml` |

### Config text surface (host-facing)

```c
char        *typio_instance_get_config_text(TypioInstance *instance);
TypioResult  typio_instance_set_config_text(TypioInstance *instance,
                                            const char *content);
```

| Function | Notes |
|----------|-------|
| `get_config_text` | Newly allocated TOML string. Caller frees with `typio_free_string`. |
| `set_config_text` | Parses `content` as TOML and replaces the live root. Equivalent to a write + reload. |

## Host-side observer callbacks

```c
void typio_instance_set_engine_changed_callback      (TypioInstance *instance,
                                                      TypioEngineChangedCallback callback,
                                                      void *user_data);
void typio_instance_set_voice_engine_changed_callback(TypioInstance *instance,
                                                      TypioVoiceEngineChangedCallback callback,
                                                      void *user_data);
void typio_instance_set_status_icon_changed_callback (TypioInstance *instance,
                                                      TypioStatusIconChangedCallback callback,
                                                      void *user_data);
void typio_instance_set_keyboard_mode_changed_callback (TypioInstance *instance,
                                                        TypioKeyboardModeChangedCallback callback,
                                                        void *user_data);
void typio_instance_set_engine_availability_changed_callback (
                                                        TypioInstance *instance,
                                                        TypioEngineAvailabilityChangedCallback callback,
                                                        void *user_data);
```

Callback signatures and pointer-lifetime rules are in
[types.md ▸ Callback typedefs](types.md#callback-typedefs).

## Engine notifications (engine-facing)

Engines call these to notify the host of state changes. They live in
`typio/abi/instance.h` so plugins can include them.

```c
void                    typio_instance_notify_status_icon   (TypioInstance *instance,
                                                             const char *icon_name);
void                    typio_instance_clear_status_icon    (TypioInstance *instance);
const char             *typio_instance_get_last_status_icon (TypioInstance *instance);

void                    typio_instance_notify_keyboard_mode (TypioInstance *instance,
                                                             const TypioKeyboardEngineMode *mode);
void                    typio_instance_clear_keyboard_mode  (TypioInstance *instance);
const TypioKeyboardEngineMode *typio_instance_get_last_keyboard_mode (TypioInstance *instance);

void                    typio_instance_notify_engine_availability (
                                                             TypioInstance *instance,
                                                             TypioEngineAvailability state,
                                                             const char *reason);
TypioEngineAvailability typio_instance_get_engine_availability (TypioInstance *instance);
```

| Function | Effect |
|----------|--------|
| `notify_status_icon` | Stores the icon name and fires the status-icon-changed callback. `icon_name` is copied. |
| `clear_status_icon` | Clears the icon; fires the callback with NULL. |
| `get_last_status_icon` | Borrowed pointer to the last notified icon name, or NULL. |
| `notify_keyboard_mode` | Stores a copy of `mode` and fires the keyboard-mode-changed callback (plus a status-icon notification derived from `mode->icon_name`). No-op if `mode->id` matches the previously notified value. |
| `clear_keyboard_mode` | Clears the last mode; fires the callback with NULL. |
| `get_last_keyboard_mode` | Borrowed pointer to the last notified mode, or NULL. |
| `notify_engine_availability` | Stores `state`, copies `reason`, and fires the engine-availability-changed callback. No-op if both state and reason are unchanged. |
| `get_engine_availability` | Last notified engine availability. Returns `TYPIO_ENGINE_READY` for NULL instance. |

## Core-internal notifications

Called by libtypio's registry machinery, not by engines. Listed for
completeness; do not call from engine or host code.

```c
void typio_instance_notify_engine_changed      (TypioInstance *instance,
                                                const TypioEngineInfo *engine);
void typio_instance_notify_voice_engine_changed(TypioInstance *instance,
                                                const TypioEngineInfo *engine);
```

## Voice session

Host-owned voice session handle; the instance carries the slot.

```c
struct TypioVoiceSession;
struct TypioVoiceSession *typio_instance_get_voice_session(TypioInstance *instance);
void                      typio_instance_set_voice_session(TypioInstance *instance,
                                                           struct TypioVoiceSession *session);
```

| Function | Notes |
|----------|-------|
| `get_voice_session` | Borrowed pointer to the registered session, or NULL |
| `set_voice_session` | Stores the host-allocated session; replaces any prior. The host retains ownership and is responsible for freeing it. NULL clears the slot. |

Full session API in `typio/runtime/voice.h` (see the runtime layer headers).

## Per-application identity

Engine/mode preferences keyed by `(provider_name, app_id)` so the active engine
and mode can be restored when focus returns to an application. Persisted
across runs in `state_dir`.

```c
bool  typio_instance_identity_preferences_enabled(TypioInstance *instance);

char *typio_instance_identity_load_engine (TypioInstance *instance,
                                           const char *provider_name,
                                           const char *app_id);
void  typio_instance_identity_store_engine(TypioInstance *instance,
                                           const char *provider_name,
                                           const char *app_id,
                                           const char *engine_name);

bool  typio_instance_identity_load_mode   (TypioInstance *instance,
                                           const char *provider_name,
                                           const char *app_id,
                                           char **out_engine,
                                           char **out_mode_id);
void  typio_instance_identity_store_mode  (TypioInstance *instance,
                                           const char *provider_name,
                                           const char *app_id,
                                           const char *mode_engine,
                                           const char *mode_id);
void  typio_instance_identity_clear_mode  (TypioInstance *instance,
                                           const char *provider_name,
                                           const char *app_id,
                                           const char *current_engine);
```

| Function | Return ownership |
|----------|------------------|
| `identity_preferences_enabled` | `true` if the host has enabled per-app persistence (config-driven) |
| `identity_load_engine` | Newly allocated engine name, or NULL if no preference. Caller frees with `typio_free_string`. |
| `identity_store_engine` | Persists `engine_name` for `(provider, app_id)`. NULL clears. |
| `identity_load_mode` | On hit: writes newly allocated `*out_engine` + `*out_mode_id` (caller frees each with `typio_free_string`) and returns `true`. On miss: returns `false`, outputs untouched. |
| `identity_store_mode` | Persists `(mode_engine, mode_id)` for `(provider, app_id)`. |
| `identity_clear_mode` | Removes the mode entry. `current_engine` is used to scope removal to the engine currently in use. |

## See also

- [Shared types](types.md) — `TypioResult`, opaque handles, callback typedefs
- [Registry](registry.md) — engine registration, activation, switching
- [Config](config.md) — `TypioConfig` operations
- [Input Context](input-context.md) — per-client input state
- [Engine ▸ Entry points](../engine/entry.md) — what `plugin_loader` calls into
- [ADR-0003](../../adr/0003-plugin-engine-abi-dual-category.md) — dual-category engine slots
