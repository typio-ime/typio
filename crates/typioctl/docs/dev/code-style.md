# Code Style

## Language versions

- Rust 2024 edition for the CLI (`typioctl`)

## Formatting

- 4-space indentation
- Keep public API names in the `typio_*` / `Typio*` style already used by the project
- Prefer small, direct functions over clever abstractions

## Documentation

- Document non-obvious behavior near complex state transitions
- Keep IPC protocol details behind the narrow `src/ipc.rs` boundary

## Design preferences

- Prefer local helpers and direct data flow over broad abstractions
- Keep module boundaries explicit

## Before submitting

- Build succeeds from a clean tree (`cargo build -p typioctl`)
- Tests pass (`cargo test -p typioctl`)
- User-facing behavior is documented
