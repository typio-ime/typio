# Package for Distribution

This document describes how to package a Typio engine for distribution through package managers, build scripts, or manual installation.

## What an engine package installs

A complete engine package installs three classes of files:

| Class | Path pattern | Description |
|-------|-------------|-------------|
| **Worker** | `${libexecdir}/typio/engines/typio-engine-<name>` | Private executable started by the host. |
| **Manifest** | `${datadir}/typio/engines/typio-engine-<name>.toml` | Metadata, engine type, capabilities, and absolute worker command. |
| **Icons** | `${datadir}/icons/hicolor/scalable/apps/typio-engine-<name>.svg` | Full-colour SVG icon. |
| **Icons** | `${datadir}/icons/hicolor/scalable/apps/typio-engine-<name>-symbolic.svg` | Symbolic SVG icon (strongly recommended). |

`${libexecdir}` and `${datadir}` are determined by the build system or
distribution policy. The host discovers manifests through its configured
`engine_dirs`.

## Build-time dependency

Engines build against the published C ABI headers shipped with `libtypio`. The canonical way to discover them is via pkg-config:

```bash
pkg-config --cflags --libs typio-engine-abi
```

This yields the include path for `typio/abi/abi.h` and the link flags for `libtypio.so`. Engines must include **only** headers under `typio/abi/`; anything under `typio/runtime/` or `typio/schema/` is host-only and off-limits.

Two `.pc` files are produced by the `libtypio` build:

| File | Purpose | Consumer |
|------|---------|----------|
| `typio-engine-abi.pc` | Headers + core library for linking. | Engine authors. |
| `libtypio.pc` | Full C ABI; requires `typio-engine-abi`. | Host integrators. |

## ABI version compatibility

The C engine ABI linked into engine executables is versioned. The `.pc` file
carries the framework version as a build-time gate. Rebuild C/C++ engines
against incompatible libtypio releases.

Packagers should ensure the engine package declares a dependency on a compatible `libtypio` version:

- **Major bump** — ABI break; engine must be rebuilt.
- **Minor bump** — additive change; existing engines continue to work.

## Icon installation

Icons must be installed to the standard XDG icon path so the host resolves
them through the normal icon theme lookup.

```text
${datadir}/icons/hicolor/
└── scalable/
    └── apps/
        ├── typio-engine-<name>.svg
        └── typio-engine-<name>-symbolic.svg
```

Ship icons as **SVG only**. A `-symbolic.svg` variant is strongly recommended: it is a single-colour SVG that GTK/Qt panels tint automatically to match the current theme, eliminating the need for multiple colour variants.

See [Engine Icon Reference](engine/icons.md) for full rules on valid and prohibited icon values.

## Packaging example (Meson)

```meson
project('typio-engine-myengine', 'c',
    version : '0.1.0',
)

typio_engine_abi = dependency('typio-engine-abi')

worker_install_dir = get_option('libexecdir') / 'typio' / 'engines'
manifest_install_dir = get_option('datadir') / 'typio' / 'engines'

engine_worker = executable('typio-engine-myengine',
    'src/engine.c',
    'src/worker_main.c',
    dependencies : typio_engine_abi,
    install : true,
    install_dir : worker_install_dir,
)

# Generate an installed manifest whose command is the absolute worker path.

icon = files('data/icons/hicolor/scalable/apps/typio-engine-myengine.svg')
icon_symbolic = files('data/icons/hicolor/scalable/apps/typio-engine-myengine-symbolic.svg')

install_data(icon,
    install_dir : get_option('datadir') / 'icons' / 'hicolor' / 'scalable' / 'apps'
)
install_data(icon_symbolic,
    install_dir : get_option('datadir') / 'icons' / 'hicolor' / 'scalable' / 'apps'
)
```

## Packaging example (Cargo)

Build native Rust workers as binaries:

```toml
[package]
name = "typio-engine-myengine"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "typio-engine-myengine"
path = "src/main.rs"
```

Install the resulting executable under `${libexecdir}/typio/engines`, generate
an installed manifest under `${datadir}/typio/engines`, and install icons via
the distribution's standard mechanisms.

## Runtime dependency

At runtime an engine package requires:

- A compatible `libtypio.so` when the worker links C ABI helper symbols.
- A host binary that discovers manifests and starts workers from the
  configured `engine_dirs`.

Workers can be protocol-smoke-tested directly, but they do not attach to the
desktop or commit text without a host.

## See also

- [Engine Naming Convention](../dev/engine-naming-convention.md) — repository, worker, manifest, and runtime name rules
- [Engine Icon Reference](engine/icons.md) — icon formats, prohibited values, and host resolution rules
- [How to Integrate a Keyboard Engine](../how-to/integrate-keyboard-engine.md) — build, install, and test checklist
- [How to Integrate a Voice Engine](../how-to/integrate-voice-engine.md) — voice engine packaging specifics
- [ADR-0004: Platform-neutral core, host-owned loading](../adr/0004-platform-neutral-core-host-loading.md) — why engine discovery lives in the host
