# Project Layout

The source tree is small and organized by function:

## `src/`

Rust source files.

- `main.rs` — CLI entry point. Parses arguments with `clap` and dispatches to subcommands.
- `commands.rs` — Subcommand implementations (`status`, `engine`, `config`, `rime`, `stop`, `version`).
- `ipc.rs` — UDS client: connects to the daemon socket, sends JSON-RPC requests, and reads responses.

## `Cargo.toml`

Crate manifest. Package: `typioctl`. Binary: `typioctl`.

## Design rationale

The CLI is intentionally minimal: it needs no C dependencies and benefits from modern argument parsing (`clap`) and error handling. The IPC boundary (`src/ipc.rs`) is the only contract with the daemon; everything else is pure client logic.
