# ADR-0002: C ABI as the Only Public Interface

- **Status**: Accepted
- **Date**: 2026-05-28
- **Deciders**: Project maintainers

## Context

`libtypio` is implemented in Rust, yet it is consumed by C/C++ hosts (`typio-wayland`), C/C++ engine plugins (`typio-engine-rime`, `typio-engine-mozc`), and potentially future hosts written in Python, Lua, or other languages. A recurring question is whether the project should also expose a native Rust crate API — traits, safe wrappers, and `cargo`-first workflows — in addition to the current C ABI.

Three structural constraints shape the answer:

1. **The primary host is C/C++.** Wayland protocol bindings, Vulkan/Flux rendering, and PipeWire integration all live in C ecosystems. A Rust-only API would force the host to write FFI glue in the wrong direction.
2. **Engines are dynamically loaded `.so` plugins.** Runtime plugin discovery requires a stable binary interface across compiler versions and toolchains. Rust has no stable ABI at the crate/dylib level — only `extern "C"` is guaranteed.
3. **Cross-language host support is a goal.** Future hosts in any language that can call C (Python, Lua, Go, …) must be possible.

## Decision

**The C ABI in `include/typio/*.h` is the sole officially supported public interface.** `libtypio` is a Rust implementation detail, not a public Rust API. The project does not publish a `libtypio` crate to crates.io and does not maintain a Rust-native API as a first-class citizen.

If the ecosystem later demands a Rust wrapper, it shall be built as a **separate, optional crate** (`libtypio-sys` for raw FFI + `libtypio` for safe wrappers) layered on top of the C ABI, not replacing it.

## Alternatives considered

- **Expose both C ABI and Rust crate API from the same crate.** Rejected: doubles the maintenance surface. Every public type, trait method, and lifetime annotation would become a stability contract maintained in lockstep.
- **Make Rust API the source of truth and auto-generate C bindings.** Rejected: the host is C/C++, so headers must remain human-readable and hand-curated. Auto-generation would produce a C API that mirrors Rust idiosyncrasies (`Option`, `Result`, ownership moves) rather than idiomatic C.
- **Use `cxx.rs` for safe Rust↔C++ interop.** Rejected: `cxx.rs` only supports C++, not pure C. The engine ABI is C, and host code contains significant C.

## Consequences

- **Positive**: one narrow, stable boundary. Hosts and engines in any language can link `libtypio.so` without knowing Rust exists.
- **Positive**: engine plugins can be written in C, C++, Rust, Zig, or any language with `extern "C"` — no toolchain lock-in.
- **Trade-off**: Rust engine authors must write `unsafe` FFI glue to implement the C vtables. Internal `Engine` / `KeyboardEngine` / `VoiceEngine` traits are invisible to them.
- **Trade-off**: a pure-Rust host cannot consume `libtypio` through idiomatic Rust APIs today. The recommended path is a community or official `libtypio-sys` + `libtypio` wrapper crate, not a rewrite of core.
- **Negative (accepted)**: the `rlib` artifact produced by Cargo is an implementation detail, not a supported distribution format.

## Future path for a Rust API

Should a Rust-native interface become necessary, the recommended architecture is:

```
libtypio-rs (safe wrapper, optional)
  └── libtypio-sys (bindgen-generated raw FFI)
      └── libtypio.so (the single stable C ABI)
```

This mirrors industrial best practice (`rusb` → `libusb`, `rusqlite` → `libsqlite3`, `git2-rs` → `libgit2`). The C ABI remains the stable foundation; the Rust layer is an additive convenience that evolves independently.

## Related

- [ADR-0003: Plugin engine ABI — dual-category slots](0003-plugin-engine-abi-dual-category.md)
- [ADR-0004: Platform-neutral core, host-owned loading](0004-platform-neutral-core-host-loading.md)
