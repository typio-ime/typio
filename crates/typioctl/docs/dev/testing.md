# Testing

This document is for contributors. It covers how to run and write tests.

## Run the test suite

```bash
cargo test -p typioctl
```

## Test ownership

Add or update tests when changing:

- IPC client behavior (`src/ipc.rs`)
- Command parsing and dispatch (`src/commands.rs`, `src/main.rs`)
- Public CLI interface or output format

## Style

- Use Rust 2024 edition.
- Keep public API names in the `typio_*` / `Typio*` style already used by the project.
- Prefer local helpers and direct data flow over broad abstractions.
- Document non-obvious behavior near complex state transitions.
