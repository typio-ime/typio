# How to Integrate a Voice Engine

This guide covers adding a new voice input engine to Typio as an external engine.

## How engines integrate

Engines are out-of-process executables declared by
`typio-engine-<name>.toml` manifests. New engines live in their own repositories
and release independently of the framework. The `<name>` token must follow
[Engine Naming Convention](../dev/engine-naming-convention.md).
For a complete, copy-pasteable starting point use [Create a Custom Voice Engine](create-custom-voice-engine.md).

The host reads `type = "voice"` from the manifest and passes that metadata to
`typio_registry_register_engine_process`. libtypio then exposes only the voice
strategy surface for that engine process.

---

## Write the engine implementation

Voice engines use the same `TypioEngineBaseOps` as keyboard engines for lifecycle management, but instead of a `TypioKeyboardEngineOps` vtable they provide a `TypioVoiceEngineOps` vtable with `process_audio`.

### Implement the engine

```c
#include "typio/abi/abi.h"

typedef struct {
    /* your state */
} MyVoiceEngineData;

static TypioResult my_voice_init(TypioEngine *engine, TypioInstance *instance) {
    MyVoiceEngineData *data = calloc(1, sizeof(MyVoiceEngineData));
    if (!data) return TYPIO_ERROR_OUT_OF_MEMORY;

    typio_engine_set_user_data(engine, data);
    /* load config from instance if needed */
    return TYPIO_OK;
}

static void my_voice_destroy(TypioEngine *engine) {
    MyVoiceEngineData *data = typio_engine_get_user_data(engine);
    free(data);
}

static void my_voice_focus_in(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    (void)ctx;
    /* Load model here if using lazy loading */
}

static void my_voice_focus_out(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    (void)ctx;
}

static void my_voice_reset(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine;
    (void)ctx;
}

static TypioResult my_voice_reload_config(TypioEngine *engine) {
    (void)engine;
    return TYPIO_OK;
}

static char *my_voice_process_audio(TypioVoiceEngine *engine,
                                     const float *samples, size_t n_samples) {
    (void)engine;
    /* Run inference, return heap-allocated text or NULL */
    return NULL;
}

static const TypioEngineInfo my_voice_info = {
    .struct_size = sizeof(TypioEngineInfo),
    .name = "my-voice",
    .display_name = "My Voice",
    .description = "Example voice engine",
    .author = "You",
    .icon = "audio-input-microphone",
    .language = "und",
    .type = TYPIO_ENGINE_TYPE_VOICE,
};

static const TypioEngineBaseOps my_voice_base_ops = {
    .init = my_voice_init,
    .destroy = my_voice_destroy,
    .focus_in = my_voice_focus_in,
    .focus_out = my_voice_focus_out,
    .reset = my_voice_reset,
    .reload_config = my_voice_reload_config,
};

static const TypioVoiceEngineOps my_voice_engine_ops = {
    .process_audio = my_voice_process_audio,
};

TypioVoiceEngine *typio_engine_create_myvoice(void) {
    return typio_voice_engine_new(&my_voice_info, &my_voice_base_ops,
                                   &my_voice_engine_ops);
}
```

Audio format contract:
- Samples are PCM float32.
- Mono, 16 kHz.
- `n_samples` is the frame count (not byte count).

### Lazy loading in `focus_in`

Voice models are large and should not be loaded at `init` time. The recommended pattern is:

- `init` — allocate state, read config paths.
- `focus_in` — load the model if not already loaded. This is synchronous: when `focus_in` returns, the model is ready (or load failed) and the voice service can begin recording immediately.
- `deactivate` — unload the model to free memory.
- `destroy` — free all remaining resources.

### Export the engine entry points

A voice engine exports `typio_voice_engine_create` and `typio_engine_get_info`. The `TYPIO_VOICE_ENGINE_DEFINE` macro does this for you:

```c
TYPIO_VOICE_ENGINE_DEFINE(my_voice_info, typio_engine_create_myvoice)
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
`typio_instance_get_engine_config(instance, "my-voice")`.

---

## Package the Worker

Use this installed layout:

```text
<prefix>/<libexecdir>/typio/engines/typio-engine-my-voice
<prefix>/<datadir>/typio/engines/typio-engine-my-voice.toml
```

```toml
name = "my-voice"
type = "voice"
protocol = "typio-engine-protocol"
command = "/usr/libexec/typio/engines/typio-engine-my-voice"
args = []
required = ["voice_input"]
optional = []
```

---

## Testing a New Engine

1. **Unit test** — If the engine has pure logic (e.g. an audio preprocessor), add tests under `tests/`.
2. **Integration test** — Run the host with the engine's manifest directory
   enabled and exercise voice input with verbose logging.
3. **Config reload test** — Change the engine's `core.toml` section and trigger reload (SIGHUP or D-Bus) to verify `reload_config` behavior.

---

## Checklist

- [ ] Engine implements required ops (`init`, `destroy`, `process_audio`).
- [ ] `struct_size` is `sizeof(TypioEngineInfo)`.
- [ ] `type` is `TYPIO_ENGINE_TYPE_VOICE`.
- [ ] Sources include only `typio/abi/abi.h` from the `typio/` tree.
- [ ] `process_audio` never blocks.
- [ ] Engine state is stored in `user_data`, not globals.
- [ ] Built as the `typio-engine-<name>` executable.
- [ ] Executable installed under `<libexecdir>/typio/engines`.
- [ ] Manifest installed under `<datadir>/typio/engines`.
- [ ] Manifest declares `type = "voice"`, `protocol = "typio-engine-protocol"`, and an absolute `command`.
- [ ] Verified through the host's engine list and debug log.
- [ ] Documentation updated: `docs/reference/engines.md`, `docs/dev/engine-naming-convention.md`, and this guide.
