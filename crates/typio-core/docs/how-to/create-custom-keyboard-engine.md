# How to Create a Custom Keyboard Engine

This guide assumes you are familiar with building Typio from source and with a
systems language that can produce an engine executable. Typio has no preferred
engine language; the host contract is Typio Engine Protocol.

## When to use this

Use this when you want to add a new keyboard input engine to Typio as an
out-of-process worker.

## Prerequisites

- A toolchain for your chosen language that can build an executable.
- The Typio engine ABI headers for C/C++ engines, or an engine protocol
  implementation for native Rust workers.

## Required exported symbols

When using the C engine ABI inside an engine executable, provide:

```c
const TypioEngineInfo *typio_engine_get_info(void);
TypioKeyboardEngine *typio_keyboard_engine_create(void);
```

## Minimal keyboard engine

```c
#include "typio/abi/abi.h"

/* ── Base operations (mandatory for every engine) ─────────────────────── */

static TypioResult my_init(TypioEngine *engine, TypioInstance *instance) {
    (void)engine;
    (void)instance;
    return TYPIO_OK;
}

static void my_destroy(TypioEngine *engine) {
    (void)engine;
}

static void my_focus_in(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    (void)ctx;
    /* No per-context state to restore. */
}

static void my_focus_out(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    /* Cancel any pending composition on focus loss. */
    typio_input_context_clear(ctx);
}

static void my_reset(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    /* Cancel any pending composition on explicit reset. */
    typio_input_context_clear(ctx);
}

static TypioResult my_reload_config(TypioEngine *engine) {
    (void)engine;
    return TYPIO_OK;
}

/* ── Keyboard operations (mandatory for keyboard engines) ─────────────── */

static TypioKeyProcessResult my_process_key(TypioKeyboardEngine *engine,
                                            TypioInputContext *ctx,
                                            const TypioKeyEvent *event) {
    (void)engine;

    if (!ctx || !event || event->type != TYPIO_EVENT_KEY_PRESS) {
        return TYPIO_KEY_NOT_HANDLED;
    }

    if (event->keysym == TYPIO_KEY_space) {
        typio_input_context_commit(ctx, "hello");
        return TYPIO_KEY_COMMITTED;
    }

    return TYPIO_KEY_NOT_HANDLED;
}

/* ── Metadata ────────────────────────────────────────────────────────── */

static const TypioEngineInfo my_info = {
    .struct_size = sizeof(TypioEngineInfo),
    .name = "my-engine",
    .display_name = "My Engine",
    .description = "Example Typio keyboard engine",
    .author = "You",
    .icon = "input-keyboard",  /* freedesktop icon name (preferred) or absolute path */
    .language = "und",
    .type = TYPIO_ENGINE_TYPE_KEYBOARD,
    .required_capabilities = NULL,  /* NULL-terminated array of capability names */
    .optional_capabilities = NULL,
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

static TypioKeyboardEngine *my_create(void) {
    return typio_keyboard_engine_new(&my_info, &my_base_ops, &my_keyboard_ops);
}

TYPIO_KEYBOARD_ENGINE_DEFINE(my_info, my_create)
```

## Build example

Link the engine implementation and engine protocol entry point into an
executable named `typio-engine-my-engine`. C and C++ implementations include
`typio/abi/abi.h` and link the libtypio engine-facing helper symbols. Native
Rust engines may implement Typio Engine Protocol directly.

## Verify and install

1. Install the executable under
   `<prefix>/<libexecdir>/typio/engines/typio-engine-my-engine`.
2. Install `typio-engine-my-engine.toml` under
   `<prefix>/<datadir>/typio/engines/`.
3. Set `type = "keyboard"`, set `protocol = "typio-engine-protocol"`, and use the executable's absolute installed path as
   `command`.
4. Restart the host and verify the engine appears in its engine list.

## Practical guidance

### Base-ops checklist (all engines)

Every engine must provide all seven base callbacks.  If your engine does not need a particular behaviour, implement it as a no-op.

| Callback | Required | Reason |
|----------|----------|--------|
| `init` | **Yes** | Allocate `user_data`, load config |
| `destroy` | **Yes** | Free resources |
| `deactivate` | **Yes** | Free large resources when switched away (or no-op) |
| `focus_in` | **Yes** | Restore UI state (or no-op) |
| `focus_out` | **Yes** | Clear UI, preserve session state |
| `reset` | **Yes** | Cancel composition on Escape |
| `reload_config` | **Yes** | Re-parse config (or no-op) |

### Keyboard-ops checklist

| Callback | Required | Reason |
|----------|----------|--------|
| `process_key` | **Yes** | Core input handling |
| `list_modes` | No | Declare available modes for UI and cycling |
| `get_active_mode` | No | Sub-mode reporting (tray icon, popup) |
| `set_active_mode` | No | Mode switching via tray/D-Bus/Shift trigger |
| `commit_candidate` | No | Host-managed candidate selection (ADR-0013) |

### Rules of thumb

- Keep engine state in `engine->user_data` or context properties.
- Do not block inside `process_key`.
- Return `TYPIO_KEY_NOT_HANDLED` for keys you want the compositor or client to keep.
- Use `typio_input_context_set_composition()` when your engine owns composition; an empty composition clears the preedit.
- The framework validates at registration time that keyboard engines provide `keyboard->process_key`.
- An engine crash terminates the engine process, not the host daemon. Validate
  inputs and return protocol errors so the host can report useful diagnostics.

## See also

- [How to Create a Custom Voice Engine](create-custom-voice-engine.md)
- [How to Integrate a Keyboard Engine](integrate-keyboard-engine.md)
- [Engine Reference](../reference/engine/index.md)
- [Architecture Overview](../explanation/architecture-overview.md) — engine manager model
