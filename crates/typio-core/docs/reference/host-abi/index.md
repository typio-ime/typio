# Host ABI Reference

The **library ABI** of `libtypio.so` — the contract consumed by hosts that
link libtypio directly. Versioned by `SONAME`; symbol visibility is controlled
by `libtypio.map`.

Read this if you are embedding libtypio in a host process (`typio`,
the control panel, an out-of-tree platform daemon). Engine authors
should read the [Engine Reference](../engine/index.md) instead.

| Page | Surface |
|------|---------|
| [Shared types](types.md) | `TypioResult`, opaque handles (`TypioInstance`, `TypioRegistry`, `TypioInputContext`, `TypioConfig`, `TypioVoiceSession`), callback typedefs |
| [Instance](instance.md) | `TypioInstance` lifecycle, directories, registry/context/config access, host callbacks, voice session, per-app identity |
| [Registry](registry.md) | `TypioRegistry` — engine registration, listing, activation, switching, commit notification |
| [Input Context](input-context.md) | `TypioInputContext` — focus, key processing, composition (`TypioPreedit`, `TypioComposition`, `TypioCandidate`), surrounding text, capabilities, callbacks |
| [Config](config.md) | `TypioConfig` — TOML load/save, typed getters/setters, array/section/key access, merge |
| [Schema](schema.md) | `TypioConfigField`, static base + dynamic engine-registered fields, `typio_config_schema_register*` / `_find` / `_apply_defaults` |
| [Event](event.md) | `TypioKeyEvent`, `TypioVoiceEvent`, `TypioEventType`, modifier helpers, key-class predicates, `TYPIO_KEY_*` constants |
| [Log](log.md) | `TypioLogger`, structured `TypioLogEvent`, ring buffer, `typio_log_<level>` macros |

## Umbrella header

Hosts include `typio/typio.h`, which pulls in the runtime layer
(`runtime/instance.h`, `runtime/registry.h`, `runtime/voice.h`), the engine
ABI (`abi/*.h`), and the schema (`schema/config_schema.h`). Cross-process
control surfaces (UDS, D-Bus) are the host's own concern — see
[ADR-0007](../../adr/0007-ipc-ownership-host-and-engine-backend-deferred.md).

## See also

- [Engine Reference](../engine/index.md) — what native C engine implementations use inside engine workers
- [Contract layers](../../dev/contract-layers.md) — header partitioning and stability rules
- [ADR-0002](../../adr/0002-c-abi-as-the-only-public-interface.md) — why C ABI is the sole public surface
