# Developer Setup

This document is for contributors who will modify Typio source code. **If you only want to use Typio**, see the [Getting Started tutorial](../tutorials/01-getting-started.md) instead.

## Requirements

- Rust toolchain (latest stable `rustc` + `cargo`)
- `pkg-config`

Optional:

- `wayland-scanner` and `wayland-protocols` — only if you are working on Wayland-protocol–related tests (rare)

Engines (rime, mozc, …) are separate projects; their dependencies (e.g.
`librime`, `protobuf`) are documented in those repositories.

## External dependencies

| Dependency | Source | Resolved version | You need to install it? |
|---|---|---|---|
| **Rust** (`libtypio`) | Cargo | latest stable | **Yes** — install `rustc` + `cargo` |

System libraries are discovered via `pkg-config`.

### Pure Rust architecture

`libtypio` is a pure Rust crate:

- **Rust** — The core library (`src/`, crate `libtypio`): instance lifecycle, config parsing and schema, input context state, engine ABI/manager/labels, key-event types, logging sink, string utilities, and Rime schema discovery.

The hand-written C headers in `include/typio/*.h` are the ABI contract — the single source of truth. Rust implements matching `#[no_mangle] pub extern "C"` functions. `cargo build` produces both the `libtypio.so` shared library and the `libtypio.rlib` Rust library artifact.

When modifying Rust code, edits are picked up automatically on the next `cargo build`.

## Clone and build

Use a debug build when editing code:

```bash
cargo build
```

For an optimized release build:

```bash
cargo build --release
```

## Run tests

```bash
cargo test
```

For an ASan build you can use the Rust nightly toolchain with `-Zsanitizer=address`:

```bash
RUSTFLAGS="-Zsanitizer=address" cargo +nightly test
```

## Optional: build the compose engine

```bash
cd ../typio-engine-compose
cargo build
```

## Project layout

See [project-layout.md](project-layout.md) for a tour of the source tree.

## Submitting changes

See the [Pull Request Checklist](../../CONTRIBUTING.md#pull-request-checklist) in `CONTRIBUTING.md`.
