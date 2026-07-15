# Engine Types Reference

Header: `typio/abi/engine.h` (engine structs and surface types) and `typio/abi/types.h` (enums and forward declarations). The umbrella `typio/abi/abi.h` pulls in both.

## Engine type tag

```c
typedef enum {
    TYPIO_ENGINE_TYPE_KEYBOARD    = 0,    /* Standard keyboard input */
    TYPIO_ENGINE_TYPE_VOICE       = 1,    /* Voice input */
    TYPIO_ENGINE_TYPE_HANDWRITING = 2,    /* Handwriting recognition */
    TYPIO_ENGINE_TYPE_CUSTOM      = 100,  /* Custom engine types start here */
} TypioEngineType;
```

`TypioEngineInfo.type` declares which category the engine belongs to and decides which active slot it occupies in `TypioRegistry`. The framework loads keyboard and voice engines into separate slots; only one of each is active at a time.

Values `1..99` are reserved for future first-class categories. Third-party engines that need a category outside the standard set should use values `>= TYPIO_ENGINE_TYPE_CUSTOM`; the host treats unknown types as opaque and refuses to install them in a known active slot.

## Metadata

```c
struct TypioEngineInfo {
    const char *name;                  /* unique engine identifier */
    const char *display_name;          /* human-readable name */
    const char *description;
    const char *author;
    const char *icon;                  /* freedesktop icon name or absolute file path */
    const char *language;              /* primary language code, e.g. "zh_CN" */
    TypioEngineType type;

    /* NULL-terminated arrays of capability name strings.
     * Either may itself be NULL (treated as empty). */
    const char *const *required_capabilities;
    const char *const *optional_capabilities;
};
```

`name` is the runtime identifier matched against config and CLI flags — it must follow the [Engine Naming Convention](../../dev/engine-naming-convention.md). `icon` is validated by the framework against the rules in [Engine Icon Reference](icons.md).

## Mode

```c
typedef struct TypioKeyboardEngineMode {
    const char *id;             /* Stable identifier: "native", "ascii", "browse" */
    const char *label;          /* Human-readable name: "Native", "ASCII" */
    const char *display_label;  /* Short badge: "中", "A", "Browse" */
    const char *icon_name;      /* Freedesktop icon name */

    /* Profile — engine-defined active profile (e.g. Rime schema) */
    const char *profile_id;     /* "luna_pinyin", "wubi86" */
    const char *profile_label;  /* "朙月拼音", "五笔86" */
    const char *description;    /* Optional detailed description */

    TypioStatusSalience salience;  /* QUIET or NOTABLE */
} TypioKeyboardEngineMode;
```

Returned by `TypioKeyboardEngineOps::get_active_mode`. The pointer (and all strings it references) must remain valid **until the next call to any engine operation on the same context** — typically a `static` table.

## Engine structs

```c
struct TypioEngine {
    const TypioEngineInfo *info;
    const TypioEngineBaseOps *base_ops;     /* mandatory */
    TypioInstance *instance;
    void *user_data;
    bool active;
    bool initialized;
    const TypioEngineSurfaceOps *surface;   /* optional */
};

struct TypioKeyboardEngine {
    struct TypioEngine base;                /* common base (offset zero) */
    const TypioKeyboardEngineOps *keyboard;
};

struct TypioVoiceEngine {
    struct TypioEngine base;                /* common base (offset zero) */
    const TypioVoiceEngineOps *voice;
};
```

`TypioEngine` is embedded as the **first member** of both `TypioKeyboardEngine` and `TypioVoiceEngine`, so a pointer to either specific type can be cast to `TypioEngine *` and back. Use this when a `TypioEngineBaseOps` callback needs the modality-specific vtable.

Field ownership:

| Field | Set by | Notes |
|---|---|---|
| `info`, `base_ops`, `keyboard` / `voice` | Engine, at `*_engine_new` | Must outlive the engine. |
| `instance` | Framework, before `init` | Engines read; never write. |
| `user_data` | Engine, via `typio_engine_set_user_data` | Engine owns and frees. |
| `active`, `initialized` | Framework | Engines read only. |
| `surface` | Engine, via `typio_engine_set_surface_ops` | Must outlive the engine. |

## Surface ops types

Engines that expose commands populate this struct and attach a
`TypioEngineSurfaceOps` vtable via `typio_engine_set_surface_ops`. Runtime
properties belong in the unified config schema, not the command surface.

```c
typedef struct {
    const char *id;                 /* stable identifier, e.g. "deploy" */
    const char *label;              /* human-readable label */
} TypioEngineCommand;
```

Command strings are engine-owned and transient: they must remain valid **until
the next call to any control op on the same engine**. The worker serialises or
copies them before issuing the next call.

## Capability negotiation

Capabilities are NULL-terminated arrays of name strings. The host advertises a supported set; engines whose `required_capabilities` is not a subset are rejected at load time. Names in `optional_capabilities` that are missing produce an info-level log entry but the engine is loaded.

Standard names (see `typio/abi/types.h`):

```
preedit
candidates
prediction
voice_input
continuous_voice
punctuation
learning
```

Names are case-sensitive snake_case. Custom names are permitted; the host treats unknown names as unsupported (and rejects the engine if an unknown name appears in `required_capabilities`).

## See also

- [Shared types](../host-abi/types.md) — `TypioResult`, opaque handles, callback typedefs (consumed by the host but referenced from engine ABI signatures)
- [Engine Operations](ops.md) — the vtables that complete each engine type
- [Entry points](entry.md) — required exports and lifecycle helpers
