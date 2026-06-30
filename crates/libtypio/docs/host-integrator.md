# Host Integrator Path

Curated reading order for anyone building a libtypio **host**, such as
`typio`, or another platform daemon.

## What a host is responsible for

- Linking `libtypio.so` and including from the `typio/typio.h` umbrella.
- Discovering `typio-engine-*.toml` manifests through the host-supplied loader
  callback.
- Registering each manifest with `typio_registry_register_engine_process`.
- Driving the per-focus `TypioInputContext` lifecycle from the platform event loop and dispatching `TypioKeyEvent` into the active engine.
- Translating engine output (composition state, commit events) into the platform's text-input protocol.
- Optionally exposing the standard D-Bus and UDS surfaces to control panels and CLI tools.

Engine lifecycle, idle policy, and switching are owned by `libtypio` — the host does not implement an "engine manager."

## Reading order

### 1. Orient

1. [Architecture Overview](explanation/architecture-overview.md) — where the host sits between the compositor, libtypio, and engines.
2. [Contract Layers](dev/contract-layers.md) — which headers a host may consume (`typio/runtime/*`, `typio/schema/*`, plus the engine ABI) and the memory-ownership rules.
3. [ADR-0017: Typio Engine Protocol](adr/0017-typio-engine-protocol.md)
   — why engines are registered as engine processes and communicate on fd 3.

### 2. Embed libtypio

Read the Host ABI Reference top-down. Every `typio_*` symbol the host calls is catalogued here.

1. [Host ABI ▸ index](reference/host-abi/index.md) — catalog of every host-facing surface.
2. [Instance](reference/host-abi/instance.md) — `TypioInstance` lifecycle, `TypioInstanceConfig`, engine-loader callback, registry/context/config access.
3. [Registry](reference/host-abi/registry.md) — registering loaded engines, listing, activation, switching, commit notification.
4. [Input Context](reference/host-abi/input-context.md) — focus, key processing, composition emission, surrounding-text, capabilities, callbacks.
5. [Event](reference/host-abi/event.md) — `TypioKeyEvent` shape, modifier helpers, `TYPIO_KEY_*` constants.
6. [Config](reference/host-abi/config.md) — TOML load/save and typed accessors.
7. [Schema](reference/host-abi/schema.md) — static base + dynamic engine-registered fields, defaults application.
8. [Log](reference/host-abi/log.md) — structured logger and ring buffer.

### 3. Cross-process surfaces (optional)

If your host exposes itself to external CLIs or control panels, that control
protocol is the host's contract. For the reference host, see the `typio`
repository's `docs/reference/`.

### 4. Deepen

Read these once the basic pipeline works.

1. [Config & Runtime Ownership](explanation/config-runtime-ownership.md) — who watches `core.toml`; who dispatches `reload_config`.
2. [Engine-Host Resource Flow](explanation/engine-host-resource-flow.md) — what flows where, in both directions.

## After this you should be able to

- Decide which `typio/*` headers your host may include and link against `libtypio.so` correctly.
- Implement the host loader callback, discover `typio-engine-*.toml`
  manifests, and register each engine argv with
  `typio_registry_register_engine_process`.
- Drive `TypioInputContext` lifecycle (focus in/out, reset, key dispatch) from your event loop.
- Forward engine composition and commit output onto your platform's text-input protocol.
- Decide which cross-process surfaces (UDS, D-Bus, none) your host exposes, knowing that contract is your repository's, not libtypio's.

## See also

- [Engine Author Path](engine-author.md) — read this too if you also write engines.
- [Glossary](reference/glossary.md) — terms used throughout the docs.
- [Project Layout](dev/project-layout.md) — source tree tour (libtypio internals; only needed if contributing to libtypio itself).
