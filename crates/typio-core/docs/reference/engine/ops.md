# Engine Operations Reference

Typio separates engine operations into four vtables:

1. **`TypioEngineBaseOps`** — mandatory for **every** engine.
2. **`TypioKeyboardEngineOps`** — mandatory only for **keyboard** engines.
3. **`TypioVoiceEngineOps`** — mandatory only for **voice** engines.
4. **`TypioEngineSurfaceOps`** — optional, for engines that expose runtime properties or commands.

Every vtable is engine-owned and **must outlive the engine instance**. The recommended pattern is a `static const` table referenced from the engine's `*_create`.

## Base operations (all engines)

```c
typedef struct TypioEngineBaseOps {
    TypioResult (*init)(TypioEngine *engine, TypioInstance *instance);
    void        (*destroy)(TypioEngine *engine);
    void        (*deactivate)(TypioEngine *engine);
    void        (*focus_in)(TypioEngine *engine, TypioInputContext *ctx);
    void        (*focus_out)(TypioEngine *engine, TypioInputContext *ctx);
    void        (*reset)(TypioEngine *engine, TypioInputContext *ctx);
    TypioResult (*reload_config)(TypioEngine *engine);
    void        (*on_config_change)(TypioEngine *engine,
                                    const char *key,
                                    const char *value);
    TypioEngineAvailability (*availability)(TypioEngine *engine);
} TypioEngineBaseOps;
```

| Callback | When called | Engine obligation |
|----------|-------------|-------------------|
| `init` | Once, after `*_engine_new`, before the engine becomes active. Receives the parent `TypioInstance`. | Allocate state via `typio_engine_set_user_data`, read config from `typio_engine_get_config_path`. Return `TYPIO_OK` on success. |
| `destroy` | Once, as part of `typio_engine_free`. The host calls `typio_engine_free`; engines never call it directly. | Free every resource allocated in `init` (including `user_data`). The struct itself is freed by the framework. |
| `deactivate` | When the user switches to another engine in the same category. The engine remains registered and may be re-`init`-ed and re-activated later. | Release large in-memory resources (voice models, dictionaries). Persist transient session state that should survive re-activation. |
| `focus_in` | The host's input context gained focus. May fire many times during the engine's lifetime. | Restore visible composition UI (preedit, candidates) hidden by the previous `focus_out`. |
| `focus_out` | The input context lost focus. | Clear visible UI but **preserve** session state (mode, conversion state) so it survives focus churn. |
| `reset` | Explicit reset — user pressed Escape, or the host called `typio_input_context_reset`. | Cancel any active composition and return to the default mode. |
| `reload_config` | The user edited the config file or invoked `typio_instance_reload_config`. | Re-parse engine-owned settings from `typio_engine_get_config_path`. Return `TYPIO_OK` even if no engine-specific config exists. |
| `on_config_change` | The host committed one engine-owned config key. | Apply a live side effect for `key` / `value`, or return without action. |
| `availability` | The host checks whether the engine can process input. | Return `TYPIO_ENGINE_READY` only when input may be routed. `NULL` means always ready. |

Lifecycle callbacks are mandatory. Optional slots are documented in the table.
Engines that do not need a mandatory callback supply a no-op (a function that
returns `TYPIO_OK` or simply returns).

The `engine` parameter is the common `TypioEngine *` base. Cast to `TypioKeyboardEngine *` or `TypioVoiceEngine *` when the callback needs modality-specific access — the base sits at offset zero in both, so the cast is safe.

## Keyboard operations

```c
typedef struct TypioKeyboardEngineOps {
    TypioKeyProcessResult (*process_key)(TypioKeyboardEngine *engine,
                                         TypioInputContext *ctx,
                                         const TypioKeyEvent *event);

    const TypioKeyboardEngineMode *(*list_modes)(TypioKeyboardEngine *engine,
                                                  size_t *count);

    const TypioKeyboardEngineMode *(*get_active_mode)(TypioKeyboardEngine *engine,
                                                       TypioInputContext *ctx);

    TypioResult (*set_active_mode)(TypioKeyboardEngine *engine,
                                    TypioInputContext *ctx,
                                    const char *mode_id);

    TypioResult (*commit_candidate)(TypioKeyboardEngine *engine,
                                     TypioInputContext *ctx,
                                     int candidate_index);
} TypioKeyboardEngineOps;
```

| Callback | Required? | When called | Notes |
|----------|-----------|-------------|-------|
| `process_key` | **Yes** | Every key event routed to the engine. | Return one of `TYPIO_KEY_NOT_HANDLED`, `TYPIO_KEY_HANDLED`, `TYPIO_KEY_COMPOSING`, `TYPIO_KEY_COMMITTED`. `NOT_HANDLED` lets the host forward the original key to the application. The four return codes describe composition state transitions; see [Composition state machine](../../explanation/composition-state-machine.md). |
| `list_modes` | No | Host queries available modes for UI (settings panel, tray menu) and to determine cycling order. | Return a static array of `TypioKeyboardEngineMode` descriptors. Write length to `*count`. Return `NULL` with `*count = 0` for engines with no mode concept. |
| `get_active_mode` | No | After `focus_in`, after activation, and whenever the host needs to refresh the tray / popup mode indicator. | The returned `TypioKeyboardEngineMode *` must remain valid **until the next call to any engine operation on the same context**. Engines typically point at a `static` table indexed by mode. Return `NULL` if the engine has no meaningful mode (the host falls back to a generic icon). |
| `set_active_mode` | No | The user requested a specific mode via the tray, D-Bus, CLI, or the standard Shift trigger. | `mode_id` is one of the `id` strings a previous `list_modes` reported for the same engine. `NULL` means "cycle to next mode". Return `TYPIO_ERROR_NOT_FOUND` for unknown ids. Only meaningful for engines that also expose mode ops. |
| `commit_candidate` | No | Host-managed candidate selection (ADR-0013). The user selected a candidate via number key (0–9), space, or enter. | The engine retrieves the candidate text at `candidate_index`, commits it via `typio_input_context_commit`. Return `TYPIO_OK` on success. Set to `NULL` if the engine handles selection internally (e.g. Rime/librime). |

The framework verifies `process_key != NULL` at registration. Mode ops and `commit_candidate` are optional; if omitted, the engine appears mode-less and handles its own candidate selection.

## Voice operations

```c
typedef struct TypioVoiceEngineOps {
    char *(*process_audio)(TypioVoiceEngine *engine,
                           const float *samples, size_t n_samples);
} TypioVoiceEngineOps;
```

| Callback | Required? | Contract |
|----------|-----------|----------|
| `process_audio` | **Yes** | Run speech-to-text on a buffer of PCM samples. |

### `process_audio` audio contract

| Aspect | Value |
|---|---|
| Sample type | `float` (single-precision) |
| Sample range | `[-1.0, +1.0]` |
| Channel layout | Mono |
| Sample rate | 16 000 Hz |
| Buffer | Caller-owned. Valid only for the call; copy what you retain. |

### `process_audio` return value

- On success: a heap-allocated, NUL-terminated UTF-8 string. **The caller frees it** with the engine's matching deallocator. The recommended convention is `malloc`/`strdup`-allocated memory so the host can `free()` it; engines that allocate from a private arena must instead ship a host-side hook (uncommon — prefer `malloc`).
- On failure or empty result: return `NULL`. Treated as "no text recognised"; the host does not raise an error.

`process_audio` runs on a host-owned inference thread, not the main loop. It may block; it must not call back into libtypio with the same `TypioInputContext` lock held.

## Surface operations (optional)

Engines that expose runtime-settable named values or invokable actions populate `TypioEngineSurfaceOps` and attach it via `typio_engine_set_surface_ops` ([entry.md](entry.md#utility-accessors)).

```c
typedef struct TypioEngineSurfaceOps {
    const TypioEngineProperty *(*list_properties)(TypioEngine *engine, size_t *out_count);
    const char                *(*get_property)(TypioEngine *engine, const char *key);
    TypioResult                (*set_property)(TypioEngine *engine, const char *key, const char *value);
    const TypioEngineCommand  *(*list_commands)(TypioEngine *engine, size_t *out_count);
    TypioResult                (*invoke_command)(TypioEngine *engine, const char *id);
} TypioEngineSurfaceOps;
```

| Callback | Contract |
|---|---|
| `list_properties` | Return a flat, engine-owned array; write its length to `*out_count`. The array and every string it references must remain valid **until the next call to any control op on the same engine**. |
| `get_property` | Return the current value (string form) for the given key, or `NULL` if unknown. Same transient-lifetime rule as `list_properties`. |
| `set_property` | Validate, apply, and persist the value. Return `TYPIO_ERROR_NOT_FOUND` for unknown keys, `TYPIO_ERROR_INVALID_ARGUMENT` for bad values, `TYPIO_OK` on success. |
| `list_commands` | Return a flat, engine-owned array of commands; same transient-lifetime rule. |
| `invoke_command` | Run the command. Return `TYPIO_ERROR_NOT_FOUND` for unknown ids, `TYPIO_OK` on success. Commands are synchronous — long-running work should be dispatched to a worker and acknowledged immediately. |

See [Types ▸ Surface types](types.md#surface-ops-types) for `TypioEngineProperty`, `TypioEngineCommand`, and `TypioEnginePropertyType`.

This vtable is what hosts, the CLI, and the control panel use to drive engine-specific knobs (e.g. Rime schema selection, `deploy` command) without per-engine code.
