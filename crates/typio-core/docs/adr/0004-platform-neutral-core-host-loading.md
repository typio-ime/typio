# ADR-0004: Platform-Neutral Core, Host-Owned Plugin Loading, Out-of-Tree Engines

- **Status**: Accepted
- **Date**: 2026-05-28
- **Deciders**: Project maintainers

## Context

Two pressures shape how `libtypio` relates to hosts and engine plugins:

1. **The core must be genuinely platform-neutral.** A future non-Linux host or an out-of-process host should be able to bring its own loading strategy without fighting the framework. That rules out core owning `dlopen` / `dlsym` / `dlclose` or knowing a hard-coded engine install directory.
2. **Engines must evolve independently.** The intent is for third parties to write engines (pinyin, SKK, handwriting, …) and for distros to package `rime` and `mozc` engines on their own release cadence — exactly as `fcitx5-rime` and `fcitx5-mozc` are packaged separately from `fcitx5`. That requires the engine ABI to exist as a published, versioned artifact, and engines to live in repositories independent of the framework.

## Decision

### 1. Core is platform-neutral; the host owns plugin loading

- Core contains no `dlopen` and no compile-time engine path. It exposes `typio_registry_register_plugin_keyboard` / `typio_registry_register_plugin_voice`: the host hands core an already-loaded plugin (factory + info function + opaque library handle + close callback) and core records it. When the registry drops the slot, it invokes the host-supplied close callback (`dlclose` on Unix, `FreeLibrary` on Windows, …).
- `TypioInstanceConfig` carries `engine_dirs` (a NULL-terminated list) and a `plugin_loader` callback. During `typio_instance_init`, core invokes the host loader once per directory.
- The host implements its loader (e.g. `hosts/wayland/plugin_loader.c`): it discovers engine `.so` files, `dlopen`s each, resolves entry points, and calls `register_plugin`. The host also owns directory resolution (CLI flag, env var, per-user XDG dir, system dir). The file name convention (`libtypio_engine_<name>.so`) is a host-level contract — the reference host uses this pattern, but alternative hosts may adopt a different scheme.

### 2. The engine ABI is a published artifact

`typio-engine-abi.pc` is installed alongside the headers so plugins discover them via pkg-config:

```
dependency('typio-engine-abi')
```

A single umbrella header `typio/abi/abi.h` gives engine authors the whole ABI in one include and nothing host-only.

### 3. Engines live in their own repositories

`typio-engine-rime`, `typio-engine-mozc`, `typio-engine-whisper`, … each build standalone against `typio-engine-abi.pc`, install `libtypio_engine_<name>.so` into `<libdir>/typio/engines`, and carry their own version. The `basic` keyboard engine remains statically linked into core as the always-present fallback.

`libtypio` keeps only the core library plus the published ABI headers. Host binaries, the CLI, the settings panel, and individual engines are each in their own repositories.

## Consequences

**Positive**

- Core is genuinely portable: a new host (X11, macOS, out-of-process) brings its own loading strategy without changing core.
- Engines release independently; distros package them separately; third parties build against a stable, discoverable ABI.
- The ABI boundary is enforced three ways: the `engine_contract` lint (engines may include only `typio/abi/`), the `.pc` version gate, and the runtime major/minor check in `register_plugin`.

**Negative / trade-offs**

- More moving parts at release time: an ABI-breaking change (major bump) requires coordinating the framework release with engine-repo updates. The major/minor policy keeps additive changes painless.
- Contributors hacking on an engine must install the framework (or point `PKG_CONFIG_PATH` at a build tree) before building the engine.

## Related

- [ADR-0003: Plugin engine ABI — dual-category slots](0003-plugin-engine-abi-dual-category.md) — the ABI itself
- [ADR-0005: Internal engine backend abstraction](0005-internal-engine-backend-abstraction.md) — how registered plugins are represented internally
