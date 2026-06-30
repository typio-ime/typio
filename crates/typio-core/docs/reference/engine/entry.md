# Engine Reference

Every native C engine implementation exposes a fixed set of C symbols to its
worker harness. The daemon itself never `dlopen`s engine code; it starts the
manifest-declared worker executable. A direct C worker links these symbols into
the executable, while a compatibility worker may resolve them with `dlsym`
inside the worker process.

The `TYPIO_KEYBOARD_ENGINE_DEFINE` / `TYPIO_VOICE_ENGINE_DEFINE` macros emit the mandatory pair with correct linkage and visibility. Use the macros unless you have a specific reason not to.

## Required exports

A native C engine exports exactly two symbols. Both have C linkage (`extern "C"` in C++) and default visibility.

### Keyboard engine

```c
const TypioEngineInfo *typio_engine_get_info(void);
TypioKeyboardEngine   *typio_keyboard_engine_create(void);
```

### Voice engine

```c
const TypioEngineInfo *typio_engine_get_info(void);
TypioVoiceEngine      *typio_voice_engine_create(void);
```

### `typio_engine_get_info`

| Aspect | Contract |
|---|---|
| When called | During worker startup or compatibility loading, before instantiation. The worker may call it again to refresh metadata; the same pointer to the same struct must be returned every time. |
| Return value | Pointer to an engine-owned `TypioEngineInfo`. Allocate it statically (e.g. as a `static const TypioEngineInfo`); never heap-allocate a fresh struct on each call. |
| Lifetime | The returned struct must remain valid for the entire engine lifetime. The worker/libtypio side copies fields it retains. |
| `struct_size` | The first field MUST be initialised to `sizeof(TypioEngineInfo)` at engine build time. The reader honours only the fields the writer's size covers, so older engines keep working across additive struct growth (see [Versioning](index.md#versioning)). |

### `typio_keyboard_engine_create` / `typio_voice_engine_create`

| Aspect | Contract |
|---|---|
| When called | Lazily, the first time the engine is activated. The worker/runtime may call it again after `typio_engine_free` if the engine is re-activated. |
| Return value | A `TypioKeyboardEngine *` / `TypioVoiceEngine *` allocated by `typio_keyboard_engine_new` / `typio_voice_engine_new` (recommended), or `NULL` on failure. |
| Failure | `NULL` is the **only** failure signal. The runtime treats `NULL` as `TYPIO_ERROR_ENGINE_LOAD_FAILED` and restores the previously active engine in the same category. Log details with `typio_log_error` before returning. |
| Threading | Called serially by the engine runtime. It is not re-entrant. |

## Optional `typio_engine_get_config_schema`

Engines that declare `engines.<name>.*` configuration fields should also
export:

```c
const TypioConfigField *typio_engine_get_config_schema(size_t *out_count);
typedef const struct TypioConfigField *(*TypioEngineConfigSchemaFunc)(size_t *out_count);
```

This export is **not** emitted by the `TYPIO_*_ENGINE_DEFINE` macros — define and export it manually.

| Behaviour | Detail |
|-----------|--------|
| Caller | The worker or compatibility loader, immediately after `typio_engine_get_info` (before instantiation) |
| Disposition | Returned fields are forwarded to `typio_config_schema_register_many` so they participate in defaulting, lookup, and UI introspection without the engine being active |
| Lifetime | The returned array, and every string reachable from it, must remain valid for as long as the plugin is loaded (typically a static); libtypio deep-copies on registration |
| Empty schema | Return NULL with `*out_count = 0`, or omit the symbol entirely |

See the [Schema reference](../host-abi/schema.md#recommended-engine-plugin-pattern)
for a Rime-style example.

Alternative: an engine that prefers eager registration can call
`typio_config_schema_register_many` directly from its `init` instead of
exporting the function — but that delays schema visibility until the engine
is instantiated, so UI consumers won't see the keys before first activation.

## Helper macros

```c
TYPIO_KEYBOARD_ENGINE_DEFINE(info_var, create_func)
TYPIO_VOICE_ENGINE_DEFINE(info_var, create_func)
```

| Argument | Must be |
|---|---|
| `info_var` | An lvalue of type `TypioEngineInfo` reachable by name at file scope. Typically a `static const TypioEngineInfo MY_INFO = { ... };`. The macro takes its address. |
| `create_func` | A `void`-argument factory returning `TypioKeyboardEngine *` (or `TypioVoiceEngine *`). The macro forwards the call from the exported symbol. |

Each macro expands to **both** exports (`typio_engine_get_info` and the matching `*_create`), each marked `extern "C"` and `TYPIO_EXPORT` so they remain visible even when the rest of the plugin is compiled `-fvisibility=hidden`.

Recommended plugin build flags:

```
-fvisibility=hidden -fvisibility-inlines-hidden
```

## Lifecycle helpers

```c
TypioKeyboardEngine *typio_keyboard_engine_new(const TypioEngineInfo        *info,
                                               const TypioEngineBaseOps     *base_ops,
                                               const TypioKeyboardEngineOps *keyboard);

TypioVoiceEngine    *typio_voice_engine_new(const TypioEngineInfo       *info,
                                            const TypioEngineBaseOps    *base_ops,
                                            const TypioVoiceEngineOps   *voice);

void                 typio_engine_free(TypioEngine *engine);
```

The host pairs each successful `*_engine_new` with exactly one `typio_engine_free`. Engines do not call `typio_engine_free` themselves.

| Argument | Lifetime |
|---|---|
| `info` | Must outlive the engine — typically the same static struct returned from `typio_engine_get_info`. |
| `base_ops` | Must outlive the engine. Typically a `static const TypioEngineBaseOps`. |
| `keyboard` / `voice` | Must outlive the engine. Typically a `static const` vtable. |

Failure: each function returns `NULL` if any argument is `NULL` or if allocation fails. If `*_create` returns `NULL`, `typio_engine_free` is **not** called — destroy any partially-built state manually before returning.

`typio_engine_free` invokes the engine's `base_ops->destroy`, releases the engine's `config_path`, and frees the struct itself. Engine `user_data` is not freed; release it from `destroy`.

## Utility accessors

```c
const char       *typio_engine_get_name(const TypioEngine *engine);
TypioEngineType   typio_engine_get_type(const TypioEngine *engine);
bool              typio_engine_has_capability(const TypioEngine *engine,
                                              const char *capability);
bool              typio_engine_is_active(const TypioEngine *engine);

const char       *typio_engine_get_config_path(const TypioEngine *engine);
void              typio_engine_set_config_path(TypioEngine *engine, const char *path);
void              typio_engine_set_user_data(TypioEngine *engine, void *data);
void              *typio_engine_get_user_data(const TypioEngine *engine);

void              typio_engine_set_surface_ops(TypioEngine *engine,
                                               const TypioEngineSurfaceOps *ops);
const TypioEngineSurfaceOps *typio_engine_get_surface_ops(const TypioEngine *engine);
```

These operate on the common `TypioEngine *` base. From inside an engine, pass `&keyboard_engine->base` or `&voice_engine->base`.

| Accessor | Notes |
|---|---|
| `typio_engine_get_name` / `_get_type` | Read-through to `TypioEngineInfo` — same lifetime as the info struct (i.e. valid for the plugin's load lifetime). |
| `typio_engine_has_capability` | Searches both `required_capabilities` and `optional_capabilities`. Returns `false` for a NULL engine or unknown name. Capability names are case-sensitive (e.g. `"preedit"`). |
| `typio_engine_set_user_data` / `_get_user_data` | The engine owns the pointer; libtypio does not free it. Set in `*_create` or `base_ops->init`; release in `base_ops->destroy`. |
| `typio_engine_set_config_path` | Called by the host before `init`. The path is duplicated; the caller may free its copy. |
| `typio_engine_set_surface_ops` | Optional control vtable for engines that expose properties / commands. Call from `*_create` or `base_ops->init`. Engines that do not call it behave as if `surface == NULL`. See [Operations ▸ Surface ops](ops.md#surface-operations-optional). |

## Minimal worked example

A keyboard plugin with no-op base ops and a passthrough `process_key`:

```c
#include <typio/abi/abi.h>

static TypioResult my_init(TypioEngine *e, TypioInstance *i)      { (void)e; (void)i; return TYPIO_OK; }
static void        my_destroy(TypioEngine *e)                     { (void)e; }
static void        my_deactivate(TypioEngine *e)                  { (void)e; }
static void        my_focus_in(TypioEngine *e, TypioInputContext *c)  { (void)e; (void)c; }
static void        my_focus_out(TypioEngine *e, TypioInputContext *c) { (void)e; (void)c; }
static void        my_reset(TypioEngine *e, TypioInputContext *c)     { (void)e; (void)c; }
static TypioResult my_reload(TypioEngine *e)                      { (void)e; return TYPIO_OK; }

static const TypioEngineBaseOps MY_BASE = {
    .init = my_init, .destroy = my_destroy, .deactivate = my_deactivate,
    .focus_in = my_focus_in, .focus_out = my_focus_out,
    .reset = my_reset, .reload_config = my_reload,
};

static TypioKeyProcessResult my_process_key(TypioKeyboardEngine *e,
                                            TypioInputContext *c,
                                            const TypioKeyEvent *ev) {
    (void)e; (void)c; (void)ev;
    return TYPIO_KEY_NOT_HANDLED;
}

static const TypioKeyboardEngineOps MY_KB = {
    .process_key = my_process_key,
    /* mode ops and commit_candidate are optional */
};

static const TypioEngineInfo MY_INFO = {
    .struct_size  = sizeof(TypioEngineInfo),
    .name         = "demo",
    .display_name = "Demo",
    .description  = "passthrough demo",
    .author       = "you",
    .icon         = "input-keyboard",
    .language     = "en",
    .type         = TYPIO_ENGINE_TYPE_KEYBOARD,
    /* capability arrays may be NULL */
};

static TypioKeyboardEngine *my_create(void) {
    return typio_keyboard_engine_new(&MY_INFO, &MY_BASE, &MY_KB);
}

TYPIO_KEYBOARD_ENGINE_DEFINE(MY_INFO, my_create)
```

Link the implementation with an engine protocol entry point into the
`typio-engine-demo` executable. Install the executable under
`<libexecdir>/typio/engines/` and its `typio-engine-demo.toml` manifest under
`<datadir>/typio/engines/`. See
[Engine Naming Convention](../../dev/engine-naming-convention.md) and
[How to Create a Custom Keyboard Engine](../../how-to/create-custom-keyboard-engine.md)
for the full integration path.
