# Project Layout

Typio is split across a Cargo workspace for the Linux host, framework, ABI
crate, and vet tool, plus separate repositories for engines and external
clients. This document covers the **ecosystem map** first, then the internal
layout of the `libtypio` crate.

## Ecosystem (repositories)

| Component | Role | Output | Depends on |
|---|---|---|---|
| **`crates/typio-core`** | platform-neutral core library + C ABI | `libtypio.so`, headers | `typio-abi` |
| **`crates/typio-abi`** | Shared `#[repr(C)]` type definitions for Rust engines and test tools | `rlib` (Rust types only) | — |
| **`crates/typio-host`** | Linux/Wayland host | `typio` binary | `libtypio`, `typio-abi`, flux |
| **`crates/typio-vet`** | Engine conformance checker | `typio-vet` binary | `typio-abi` |
| **typioctl** | command-line client | `typioctl` binary | nothing (UDS only) |
| **typio-settings** | flux-ui preferences panel | `typio-settings` binary | `libtypio` headers + shared lib + flux-ui |
| **typio-engine-compose** | Latin keyboard engine with compose picker (optional; framework runs with zero engines) | `typio-engine-compose` executable + manifest | Typio Engine Protocol |
| **typio-engine-rime** | Rime engine | `typio-engine-rime` executable + manifest | `libtypio` headers + librime |
| **typio-engine-mozc** | Mozc engine | `typio-engine-mozc` executable + manifest | `libtypio` headers + protobuf |
| **typio-engine-sherpa** | Sherpa-ONNX voice engine | `typio-engine-sherpa` executable + manifest | `libtypio` headers + sherpa-onnx |
| **typio-engine-whisper** | Whisper voice engine | `typio-engine-whisper` executable + manifest | `libtypio` headers + whisper.cpp |
| **`examples/engine-template/`** (in this repo) | Minimal hello-world engine starter | `typio-engine-hello` executable + manifest | `libtypio` headers |
| **typio-docs** | Documentation site | static HTML | nothing |

Engine repositories must follow the naming rules in [Engine Naming Convention](engine-naming-convention.md).

The rule the split encodes is **core owns business logic; hosts own
platform glue; engines are out-of-process engine processes discovered at runtime**. libtypio
knows nothing about Wayland, D-Bus, GTK, X11, Vulkan, or the event loop.
A host knows everything about its platform but delegates all linguistic
and configuration decisions to libtypio. Engines are discovered by the host
from manifests under `<datadir>/typio/engines`, registered with libtypio, and
run as workers under `<libexecdir>/typio/engines`. Neither core nor the host
contains per-engine code (see [ADR-0004](../adr/0004-platform-neutral-core-host-loading.md)).

The C ABI in `include/typio/` is the narrow boundary. Engines compile
against only `typio/abi/`; hosts and the control panel additionally use
`typio/runtime/` and `typio/schema/` (see
[contract-layers.md](contract-layers.md)). The cross-process engine
protocol (fd-3 framed IPC) is defined by `include/typio/abi/engine_protocol.h`
and owned by this repository; the host-side UDS control surface (TIP v1)
lives in the host crate — see
[ADR-0007](../adr/0007-ipc-ownership-host-and-engine-backend-deferred.md).

## This crate (`libtypio`)

### Crate root

The core library — a Rust crate (`libtypio`) exposing a hand-written C
ABI, plus its public headers.

- `Cargo.toml` / `Cargo.lock` — crate manifest. Package: `libtypio`. Outputs: `libtypio.so` (cdylib) and `libtypio.rlib`.
- `build.rs` — build script. Generates the `libtypio.pc` and `typio-engine-abi.pc` pkg-config files into `target/<profile>/`, and applies the GNU ld version script (`libtypio.map`) on ELF targets to restrict exported symbols to the `typio_*` / `TYPIO_*` namespace. The C headers under `include/typio/` are hand-written.
- `libtypio.map` — linker version script controlling exported C ABI symbols.
- `src/` — Rust sources. Each top-level module corresponds to one ABI area; larger modules are split into a directory of submodules.
  - `lib.rs` — crate root; declares modules and the crate-wide `clippy::not_unsafe_ptr_arg_deref` allowance for the C ABI boundary.
  - `config.rs` + `config/` (`parse.rs`, `serialize.rs`, `getters.rs`, `setters.rs`) — TOML config load/save and typed accessors.
  - `config_schema.rs` — static schema describing config keys, types, and defaults.
  - `input_context.rs` + `input_context/` (`callbacks.rs`, `content.rs`, `focus.rs`) — input context state, surrounding-text content, focus tracking, host callbacks.
  - `instance.rs` + `instance/` (`identity.rs`, `context.rs`, `callbacks.rs`, `config_ops.rs`) — top-level Typio instance lifecycle and per-instance config operations.
  - `engine/mod.rs` — public engine trait and FFI vtable plumbing exposed to plugins.
  - `core/engine/` (`mod.rs`, `backend/`, `event.rs`, `mode.rs`) — pure-Rust engine trait and `EngineBackend` abstraction ([ADR-0005](../adr/0005-internal-engine-backend-abstraction.md)). The only backend today is `Process` (out-of-process workers speaking the Typio Engine Protocol over fd 3); the in-process `FfiEngine` was removed in 0.2.0.
  - `core/engine/backend/` (`mod.rs`, `process.rs`, `engine_protocol.rs`) — the out-of-process worker transport and framed-IPC primitives.
  - `core/registry/` (`mod.rs`, `policy.rs`) — `EngineRegistry`, idle policy, switching.
  - `c_api/` (`mod.rs`, `registry.rs`) — C ABI surface (`typio_registry_*`); thin translation layer over `core::registry`.
  - `event.rs`, `types.rs` — key-event types and ABI primitives.
  - `log.rs`, `string.rs` — logging sink, string allocator, and deallocator family (C ABI).
  - `shortcut.rs` — action-keyed shortcut bindings.
  - `voice/` (`session.rs`, `audio.rs`, `state.rs`, `types.rs`, `mod.rs`) — host-owned voice session (created by host, dispatch via registry).
  - `integration_tests.rs` — in-crate integration tests covering the C ABI surface.
- `include/typio/` — installed public C headers (single source of truth), partitioned by contract layer:
  - `typio.h` — umbrella header re-exporting the stable surface.
  - `abi/` — engine-facing ABI (`abi.h`, `engine.h`, `engine_protocol.h`, `event.h`, `types.h`, `version.h`, `config.h`, `input_context.h`, `instance.h`, `log.h`, `shortcut.h`, `string.h`, `voice.h`).
  - `runtime/` — host-facing runtime APIs (`instance.h`, `registry.h`, `voice.h`).
  - `schema/` — config schema introspection (`config_schema.h`).

### `docs/`

Ecosystem documentation hub: architecture, the engine ABI and contract
layers, how-to guides, and the ADRs. Consumer repos carry only a README
and point back here.

## Design rationale

The core is a Rust crate to get memory and concurrency safety on the
parts most easily corrupted; its C ABI keeps every consumer free to
remain C/C++/Rust without churn. Public headers live next to the crate so
consumers see a single include root (`include/typio/`), shipped to them
via the installed headers.
