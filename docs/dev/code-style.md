# Code Style

## Language versions

- Rust 2024 edition (Rust 1.85+), per the workspace `Cargo.toml`
- C only for libtypio's framework core and the FFI ABI surface; new host
  code is Rust (see [ADR-0035](../adr/0035-bilingual-migration-to-rust.md),
  [ADR-0038](../adr/0038-framework-abi-vet-monorepo.md))

## Formatting

- 4-space indentation
- Keep public API names in the `typio_*` / `Typio*` style on the C/FFI
  surface; in Rust, follow `cargo fmt` defaults and idiomatic `snake_case`
  modules / `CamelCase` types
- Prefer small, direct functions over clever abstractions

## Documentation

- Document non-obvious behavior in module (`//!`) or item doc comments,
  especially around complex state transitions
- Keep generated protocols and renderer details behind narrow module boundaries

## Design preferences

- Prefer local helpers and direct data flow over broad abstractions
- Keep module boundaries explicit

## Before submitting

- Build succeeds from a clean tree
- `cargo test -p typio-host -p typio-core -p typio-abi -p typio-vet -p typioctl`
  passes
- User-facing behavior is documented
- Any new engine or runtime assumptions are written down
