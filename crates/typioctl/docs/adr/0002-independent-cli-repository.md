# ADR-0002: Independent CLI repository and complete separation from daemon

- **Status**: Accepted
- **Date**: 2026-05-26
- **Deciders**: Project maintainers

## Context

The `typio` command-line client was historically part of the same repository as the Typio daemon (`typiod`). This arrangement mixed unrelated concerns:

- The daemon links heavy platform dependencies: Wayland, Vulkan, XKB, PipeWire, D-Bus, GTK, FreeType, HarfBuzz.
- The client needs none of these; it only talks to the daemon over a Unix Domain Socket.
- CLI features (colored output, structured logging, shell completion) are much easier to build in Rust than in C.
- Every daemon bugfix or feature release forced a client redeploy, and vice versa.

## Decision

The CLI exists as an **independent Rust repository** with clear boundaries:

1. **`typioctl`** (Rust, this repository)
   - Pure client. Only communicates with an already-running daemon.
   - If the daemon is not running, prints an error and exits.
   - Never attempts to locate, fork, or exec `typiod`.
   - No dependency on any C platform libraries.

2. **`typiod`** (C, separate project)
   - The background service binary. Started by systemd, desktop autostart, or manual invocation.
   - Owns the UDS socket (`$XDG_RUNTIME_DIR/typio/daemon.sock`).

The IPC protocol (method names, property names, socket path convention) is a public contract. Both sides use JSON-RPC 2.0 with a 4-byte big-endian length prefix.

## Alternatives considered

- **Keep the CLI inside the monorepo**: Rejected. It blocks CLI improvements and forces client users to install daemon dependencies.
- **Put the CLI inside the core library as a second binary target**: Rejected. The core is the *library* crate; adding a CLI binary there would couple the client to the core build and confuse the dependency graph.
- **Embed daemon logic into the CLI via FFI**: Rejected. It would force the CLI to link Wayland, Vulkan, D-Bus, etc., defeating the purpose of the split.
- **Use systemd socket activation**: Rejected. Over-engineered for the current scope and not portable to non-systemd environments.

## Consequences

- Positive: CLI can evolve independently (clap derive, serde_json, tracing, etc.).
- Positive: Packaging flexibility — headless servers can skip the CLI, management scripts can skip the daemon.
- Positive: Cleanest possible separation of concerns. CLI is pure client; daemon is pure server.
- Trade-off: The IPC protocol is now a public contract. Changes must be coordinated across the daemon and the CLI.
- Negative (accepted): Users must start the daemon themselves (or rely on systemd/autostart) before using `typioctl` commands.
