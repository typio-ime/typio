# How to Integrate a Keyboard Engine

This guide covers adding a new keyboard input engine to Typio as an external engine.

## How engines integrate

Engines are out-of-process executables declared by
`typio-engine-<name>.toml` manifests. New engines live in their own repositories
and release independently of the framework. The `<name>` token must follow
[Engine Naming Convention](../dev/engine-naming-convention.md).
For a complete, copy-pasteable starting point use [Create a Custom Keyboard Engine](create-custom-keyboard-engine.md).

The host reads `type = "keyboard"` from the manifest and passes that metadata
to `typio_registry_register_engine_process`. libtypio then exposes only the
keyboard strategy surface for that engine process.

---

## Write the engine implementation

A minimal keyboard engine needs `init`, `destroy`, and `process_key`. See [`typio-engine-compose/src/lib.rs`](../../typio-engine-compose/src/lib.rs)
for a concise reference (it is a Rust engine, but the C ABI callback
shapes are identical for C engines).

Key rules:

- Return `TYPIO_KEY_NOT_HANDLED` for keys you do not consume.
- Do not block inside `process_key`.
- Store engine-specific state in `engine->user_data`.
- Use `typio_input_context_set_composition()` (preedit + candidates in one `TypioComposition`) and `typio_input_context_commit()` to output text ([ADR-0006](../adr/0006-composition-state-and-commit-event.md); pre-migration code still uses `set_preedit()` / `set_candidates()`).

Example skeleton:

```c
#include "typio/abi/abi.h"

typedef struct {
    /* your state */
} MyEngineData;

static TypioResult my_init(TypioEngine *engine, TypioInstance *instance) {
    MyEngineData *data = calloc(1, sizeof(MyEngineData));
    if (!data) return TYPIO_ERROR_OUT_OF_MEMORY;

    /* load config from instance if needed */
    typio_engine_set_user_data(engine, data);
    return TYPIO_OK;
}

static void my_destroy(TypioEngine *engine) {
    MyEngineData *data = typio_engine_get_user_data(engine);
    free(data);
}

static void my_focus_in(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    (void)ctx;
}

static void my_focus_out(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    (void)ctx;
}

static void my_reset(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    (void)ctx;
}

static TypioResult my_reload_config(TypioEngine *engine) {
    (void)engine;
    return TYPIO_OK;
}

static TypioKeyProcessResult my_process_key(TypioKeyboardEngine *engine,
                                            TypioInputContext *ctx,
                                            const TypioKeyEvent *event) {
    /* ... handle key ... */
    return TYPIO_KEY_NOT_HANDLED;
}

static const TypioEngineInfo my_info = {
    .struct_size = sizeof(TypioEngineInfo),
    .name = "myengine",
    .display_name = "My Engine",
    .description = "Example keyboard engine",
    .author = "You",
    .icon = "input-keyboard",
    .language = "und",
    .type = TYPIO_ENGINE_TYPE_KEYBOARD,
};

static const TypioEngineBaseOps my_base_ops = {
    .init = my_init,
    .destroy = my_destroy,
    .focus_in = my_focus_in,
    .focus_out = my_focus_out,
    .reset = my_reset,
    .reload_config = my_reload_config,
};

static const TypioKeyboardEngineOps my_keyboard_ops = {
    .process_key = my_process_key,
};

/* Built-ins export info/create directly; no macro needed */
const TypioEngineInfo *typio_engine_get_info_myengine(void) {
    return &my_info;
}

TypioKeyboardEngine *typio_engine_create_myengine(void) {
    return typio_keyboard_engine_new(&my_info, &my_base_ops, &my_keyboard_ops);
}
```

### Export the engine entry points

A keyboard engine exports `typio_keyboard_engine_create` and `typio_engine_get_info`. The
`TYPIO_KEYBOARD_ENGINE_DEFINE` macro does this for you:

```c
TYPIO_KEYBOARD_ENGINE_DEFINE(my_info, typio_engine_create_myengine)
```

### Build and install

Link the engine implementation and engine protocol entry point into
`typio-engine-<name>`. Install the executable under
`<libexecdir>/typio/engines/` and its manifest under
`<datadir>/typio/engines/`. The installed manifest must use an absolute
`command` path to the executable.

### Add config schema (optional)

If your engine reads configuration from `core.toml`, document the keys
it consumes. Engines read their section via
`typio_instance_get_engine_config(instance, "myengine")`.

---

## Package the Worker

Use this installed layout:

```text
<prefix>/<libexecdir>/typio/engines/typio-engine-my-engine
<prefix>/<datadir>/typio/engines/typio-engine-my-engine.toml
```

```toml
name = "my-engine"
type = "keyboard"
protocol = "typio-engine-protocol"
command = "/usr/libexec/typio/engines/typio-engine-my-engine"
args = []
required = ["preedit", "candidates"]
optional = []
```

---

## Testing a New Engine

1. **Unit test** — If the engine has pure logic (e.g. a key parser), add tests under `tests/`.
2. **Integration test** — Run the host with the engine's manifest directory
   enabled and exercise key sequences with verbose logging.
3. **Config reload test** — Change the engine's `core.toml` section and trigger reload (SIGHUP or D-Bus) to verify `reload_config` behavior.

---

## Checklist

- [ ] Engine implements required ops (`init`, `destroy`, `process_key`).
- [ ] `struct_size` is `sizeof(TypioEngineInfo)`.
- [ ] `type` is `TYPIO_ENGINE_TYPE_KEYBOARD`.
- [ ] Sources include only `typio/abi/abi.h` from the `typio/` tree.
- [ ] `process_key` never blocks.
- [ ] Engine state is stored in `user_data`, not globals.
- [ ] Built as the `typio-engine-<name>` executable.
- [ ] Executable installed under `<libexecdir>/typio/engines`.
- [ ] Manifest installed under `<datadir>/typio/engines`.
- [ ] Manifest declares `type = "keyboard"`, `protocol = "typio-engine-protocol"`, and an absolute `command`.
- [ ] Verified through the host's engine list and debug log.
- [ ] Documentation updated: `docs/reference/engines.md`, `docs/dev/engine-naming-convention.md`, and this guide.
