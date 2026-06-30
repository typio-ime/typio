# typio-abi

Shared C ABI type definitions for the Typio input method framework.

This crate contains **only** `#[repr(C)]` structs, enums, constants, and
opaque zero-size handles. It has **no implementation code**, **no
platform dependencies**, and **no runtime**. Its sole purpose is to give
Rust consumers a single source of truth for the layout of values that
cross the C FFI boundary between the Typio host and engine plugins.

## Who should use this

| Consumer | Should depend on `typio-abi`? |
|---|---|
| **Rust engine plugins** (e.g. `typio-engine-basic`) | **Yes** — import types instead of replicating them by hand. |
| **`typio-engine-test`** and other test/lint tools | **Yes** — mock harnesses need the same layouts. |
| **Hosts embedding `libtypio`** | No — link `libtypio` directly; it already contains these types internally. |
| **C/C++ engines** | No — include the C headers under `libtypio/include/typio/abi/` instead. |

## What is included

- Result codes (`TypioResult`)
- Engine metadata and vtables (`TypioEngineInfo`, `TypioEngine`, `TypioEngineBaseOps`, `TypioKeyboardEngineOps`, `TypioVoiceEngineOps`, `TypioEngineSurfaceOps`)
- Event types (`TypioEventType`, `TypioKeyEvent`, `TypioModifier`, key symbol constants)
- Composition types (`TypioPreeditFormat`, `TypioPreeditSegment`, `TypioComposition`, `TypioCandidate`)
- Opaque handles (`TypioInputContext`, `TypioInstance`, `TypioRegistry`, `TypioVoiceSession`)
- Callback typedefs and log types

**Not included** (by design):

- `TypioAbiVersion` — each engine defines its own ABI version export; the
  host verifies it at load time.
- Any `extern "C"` function declarations — those belong to the host
  runtime (`libtypio`) or to per-project mock harnesses.

## Usage

Add to your engine's `Cargo.toml`:

```toml
[dependencies]
typio-abi = { path = "../typio/crates/typio-abi" }
```

In your engine source:

```rust
use typio_abi::*;
```

Then implement the vtables and export the required entry points exactly
as you would when including the C headers.

## Relationship to `libtypio`

`libtypio` is the **host runtime** — it implements input contexts,
instance management, engine loading, and the full C ABI surface.
`typio-abi` is a **subset** extracted from `libtypio` so that Rust
engines do not need to link the entire host library just to agree on
struct layouts.

The C headers in `libtypio/include/typio/abi/` and the Rust types in this
crate are maintained as a single logical definition. If one changes, the
other must be updated to match.

## Build

```bash
cargo build
```

There are no tests in this crate (it contains only type definitions).
Downstream crates (`typio-engine-basic`, `typio-engine-test`, `libtypio`)
verify correctness through their own test suites.

## License

MIT — same as `libtypio`.
