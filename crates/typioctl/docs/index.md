# typioctl Documentation

`typioctl` is the command-line client for the [Typio](https://github.com/) input method framework. It controls a running `typio` host over a Unix Domain Socket (UDS) JSON-RPC protocol.

## Sections

- **[How-to Guides](how-to/)** — Task-oriented recipes for specific goals.
  - [Communicate over UDS](how-to/communicate-over-uds.md) — Send JSON-RPC commands to the daemon via its Unix Domain Socket.
- **[Reference](reference/)** — Lookup-oriented documentation.
  - [CLI Reference](reference/command.md) — Command-line flags and `typioctl` client subcommands
  - [IPC Protocol Reference](reference/ipc-protocol.md) — UDS socket path, JSON-RPC wire format, methods, and properties
- **[Architecture Decisions](adr/)** — Immutable records of past design decisions.
- **[Developer Documentation](dev/)** — Contributor-oriented docs.
  - [Developer Setup](dev/setup.md)
  - [Testing](dev/testing.md)
  - [Code Style](dev/code-style.md)
  - [Project Layout](dev/project-layout.md)

## Quick Links

- [README](../README.md) — Project pitch and quick start
- [Contributing](../CONTRIBUTING.md) — How to contribute
