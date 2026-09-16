# Code Style

## Language versions

- Rust 2024 edition (Rust 1.85+), per the workspace `Cargo.toml`
- Rust for the daemon, runtime, protocol, manifest, vet, and workspace clients.
  Native C remains only in external libraries and engines that implement their
  worker internally in C or C++.

## Formatting

- 4-space indentation
- Follow `cargo fmt` defaults and idiomatic `snake_case` modules /
  `CamelCase` types. Protocol wire spellings are defined by the typed codec and
  must not be inferred from Rust layout.
- Prefer small, direct functions over clever abstractions

## Documentation

- Document non-obvious behavior in module (`//!`) or item doc comments,
  especially around complex state transitions
- Keep generated protocols and renderer details behind narrow module boundaries
- Keep IPC protocol details behind the narrow IPC boundary of the CLI crate
  (`crates/typio-control`: the local `src/ipc.rs` of the earlier layout, now the
  shared `typio-client` crate) instead of inside command handlers

## Design preferences

- Prefer local helpers and direct data flow over broad abstractions
- Keep module boundaries explicit

## Before submitting

- Build succeeds from a clean tree
- `cargo test -p typio-daemon -p typio-runtime -p typio-engine-protocol
  -p typio-engine-manifest -p typio-engine-check -p typio-control` passes
- User-facing behavior is documented
- Any new engine or runtime assumptions are written down
