# ADR-0005: Project and Package Naming

- **Status**: Accepted
- **Date**: 2026-06-04
- **Deciders**: ming2k

## Context

ADR-0003 named the installed CLI binary `typioctl` but kept the repository and
Cargo package under `typio-cli`. The Wayland host now owns the `typio` command
name, so the CLI project needs a distinct project name as well as a distinct
binary name.

Keeping `typio-cli` in the manifest and docs creates three names for one tool:
the repository path, the Cargo package, and the installed binary. That makes
packaging and cross-repository documentation harder to keep consistent.

## Decision

The CLI project, Cargo package, and installed binary are named `typioctl`.

- The Cargo package is `typioctl`.
- The installed binary remains `typioctl`.
- The Wayland host owns the `typio` command.
- References to the old `typio-cli` project name are removed from active docs.

## Alternatives Considered

- **Keep `typio-cli` as the package name**: rejected because it preserves a
  stale project identity after the binary and repository moved to `typioctl`.
- **Rename the CLI binary back to `typio`**: rejected because the Wayland host
  now provides the primary `typio` command.

## Consequences

- Positive: package metadata, user docs, and the installed command use one name.
- Positive: `typio` remains available for the Wayland host daemon.
- Trade-off: downstream package recipes that refer to the old Cargo package name
  must update to `typioctl`.
