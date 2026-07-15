# How to Create a Custom Voice Engine

This guide assumes you are familiar with building Typio from source and with a
systems language that can produce an engine executable. Typio has no preferred
engine language; the host contract is Typio Engine Protocol.

## When to use this

Use this when you want to add a new voice input engine to Typio as an
out-of-process worker.

## Prerequisites

- A toolchain for your chosen language that can build an executable.
- The Typio engine ABI headers for C/C++ engines, or an engine protocol
  implementation for native Rust workers.

## Required exported symbols

When using the C engine ABI inside an engine executable, provide:

```c
const TypioEngineInfo *typio_engine_get_info(void);
TypioVoiceEngine *typio_voice_engine_create(void);
```

## Minimal voice engine

Voice engines provide base operations **and** a `TypioVoiceEngineOps` vtable:

```c
#include "typio/abi/abi.h"

/* ── Base operations (optional lifecycle callbacks) ───────────────────── */

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

/* ── Voice operations (mandatory for voice engines) ───────────────────── */

static char *my_process_audio(TypioVoiceEngine *engine,
                               const float *samples, size_t n_samples) {
    (void)engine;
    (void)samples;
    (void)n_samples;
    /* Run inference, return heap-allocated text or NULL */
    return NULL;
}

/* ── Metadata ────────────────────────────────────────────────────────── */

static const char *const my_voice_required[] = { "voice_input", NULL };

static const TypioEngineInfo my_info = {
    .name = "my-voice",
    .display_name = "My Voice",
    .description = "Example Typio voice engine",
    .author = "You",
    .icon = "audio-input-microphone",  /* freedesktop icon name (preferred) or absolute path */
    .language = "und",
    .type = TYPIO_ENGINE_TYPE_VOICE,
    .required_capabilities = my_voice_required,
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

static const TypioVoiceEngineOps my_voice_ops = {
    .process_audio = my_process_audio,
};

static TypioVoiceEngine *my_create(void) {
    return typio_voice_engine_new(&my_info, &my_base_ops, &my_voice_ops);
}

TYPIO_VOICE_ENGINE_DEFINE(my_info, my_create)
```

Audio format contract:
- Samples are PCM float32.
- Mono, 16 kHz.
- `n_samples` is the frame count (not byte count).

## Build example

Link the engine implementation and engine protocol entry point into an
executable named `typio-engine-my-voice`. C and C++ implementations include
`typio/abi/abi.h`; engine construction helpers are header-only, while the
worker links libtypio for its local instance, input-context, logging, and
protocol support. Native Rust engines may implement Typio Engine Protocol
directly.

## Verify and install

1. Install the executable under
   `<prefix>/<libexecdir>/typio/engines/typio-engine-my-voice`.
2. Install `typio-engine-my-voice.toml` under
   `<prefix>/<datadir>/typio/engines/`.
3. Set `type = "voice"`, set `protocol = "typio-engine-protocol"`, and use the executable's absolute installed path as
   `command`.
4. Restart the host and verify the engine appears in its engine list.

## Practical guidance

### Base-ops checklist (all engines)

The base vtable is required, but lifecycle slots may be `NULL`. Implement a
callback when the engine owns the corresponding resource or behavior.

| Callback | Required | Reason |
|----------|----------|--------|
| `init` | No | Allocate `user_data`, load config |
| `destroy` | No | Free resources allocated by `init` |
| `deactivate` | No | Free large resources when switched away |
| `focus_in` | No | Restore UI state |
| `focus_out` | No | Clear UI while preserving session state |
| `reset` | No | Cancel composition on Escape |
| `reload_config` | No | Re-read worker-local unified config |
| `on_config_change` | No | React to one engine-owned key |
| `availability` | No | Report asynchronous readiness; NULL means Ready |

### Voice-ops checklist

| Callback | Required | Reason |
|----------|----------|--------|
| `process_audio` | **Yes** | Core audio inference |

### Lazy loading in `focus_in`

Voice models are large and should not be loaded at `init` time. The recommended pattern is:

- `init` — allocate state, read config paths.
- `focus_in` — load the model if not already loaded. This is synchronous: when `focus_in` returns, the model is ready (or load failed) and the voice service can begin recording immediately.
- `deactivate` — unload the model to free memory.
- `destroy` — free all remaining resources.

### Rules of thumb

- Keep engine state in `engine->user_data` or context properties.
- Do not block inside `process_audio`.
- Return heap-allocated text from `process_audio`; the framework takes ownership.
- The framework validates at registration time that voice engines provide `voice->process_audio`.

## See also

- [How to Create a Custom Keyboard Engine](create-custom-keyboard-engine.md)
- [How to Integrate a Voice Engine](integrate-voice-engine.md)
- [Engine Reference](../reference/engine/index.md)
- [Architecture Overview](../explanation/architecture-overview.md) — engine manager model
