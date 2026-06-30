# Engine Reference

The contract that C engine implementations use inside an out-of-process
engine worker. The daemon does not `dlopen` engines; it starts manifest-declared
worker executables and libtypio speaks Typio Engine Protocol over fd 3. C
workers may link this ABI directly, or a compatibility worker may load a
legacy `.so` internally. The ABI evolves additively: structs that the engine
allocates open with a `struct_size` sentinel (ADR-0003) and capability strings
advertise optional behaviour.

Read this if you are authoring a keyboard or voice engine. Hosts
embedding libtypio should read the [Host ABI Reference](../host-abi/index.md)
instead.

The distinction between *engine* and *worker packaging* is deliberate
([ADR-0003](../../adr/0003-plugin-engine-abi-dual-category.md)):

- **Engine** — any implementation of the `TypioEngine` vtable.
- **Worker** — an executable package declared by a `typio-engine-*.toml`
  manifest. The worker may contain a native C engine, a Rust engine, or a
  bridge to a legacy loadable object.

| Page | Surface |
|------|---------|
| [Entry points](entry.md) | Required exports (`typio_engine_get_info`, `typio_keyboard_engine_create` / `typio_voice_engine_create`), `TYPIO_KEYBOARD_ENGINE_DEFINE` / `TYPIO_VOICE_ENGINE_DEFINE` macros, lifecycle helpers, utility accessors |
| [Types](types.md) | `TypioEngineInfo`, `TypioKeyboardEngineMode`, `TypioEngine`, `TypioKeyboardEngine`, `TypioVoiceEngine`, capability names |
| [Operations](ops.md) | `TypioEngineBaseOps`, `TypioKeyboardEngineOps`, `TypioVoiceEngineOps` vtables |

## Umbrella header

Native C engine implementations include `typio/abi/abi.h`, which pulls in the entire engine ABI
(`abi/engine.h`, `abi/event.h`, `abi/input_context.h`, `abi/types.h`,
`abi/voice.h`, etc.). Engine implementation code MUST NOT include anything
from `typio/runtime/` or `typio/schema/` — see
[contract layers](../../dev/contract-layers.md).

## Versioning

| Mechanism | Witness | Notes |
|-----------|---------|-------|
| `typio_engine_abi_version` | Native C engine implementation | Engine metadata and vtable compatibility; major must match, engine minor must not exceed runtime minor. |
| `struct_size` first field | `TypioKeyEvent`, `TypioComposition` | Caller-allocated payload structs grow additively; set the field to `sizeof(...)` at build time. The reader honours only the fields the writer's size covers. |
| Capability negotiation | `required_capabilities` / `optional_capabilities` on `TypioEngineInfo` (NULL-terminated string arrays) | Any required capability not in the host's supported set → engine rejected at load. Optional capability missing → info-level log, engine still loaded. |

## See also

- [Host ABI ▸ Shared types](../host-abi/types.md) — `TypioResult` and other ABI types shared with the host surface
- [How to Create a Custom Keyboard Engine](../../how-to/create-custom-keyboard-engine.md)
- [How to Create a Custom Voice Engine](../../how-to/create-custom-voice-engine.md)
- [Engine Naming Convention](../../dev/engine-naming-convention.md)
- [ADR-0003](../../adr/0003-plugin-engine-abi-dual-category.md) — Plugin engine ABI with dual-category slots
- [Engine Contract](../../explanation/engine-contract.md) — design rationale
