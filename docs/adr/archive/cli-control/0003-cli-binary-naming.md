# ADR-0003: CLI binary naming

- **Status**: Superseded by ADR-0005
- **Date**: 2026-05-26
- **Deciders**: ming2k

## Context

When the CLI was part of the monorepo, it was natural to name the binary `typio` (the user-facing entry point) while the daemon was `typiod`. After extracting the CLI into its own repository, a branding concern emerged:

- The `typio` brand is the overall project name, not a single component.
- The CLI repository (`typio-cli`) risks claiming the primary brand while the daemon (`typiod`) is the actual runtime.
- If the daemon is ever extracted to its own repository, naming conflicts become unavoidable.

## Decision

The CLI binary is named **`typioctl`**.

- The repository remains `typio-cli`.
- The installed binary is `typioctl`.
- The daemon binary remains `typiod`.
- The `typio` name is reserved for the overall project brand.

## Alternatives considered

- **`typio`**: Rejected. It conflates the project brand with a single component and blocks future daemon-repository naming.
- **`typioct`**: Rejected. Pronunciation and meaning are unclear.
- **`typio-settings` / `typio-config`**: Rejected. The CLI does more than settings (engine switching, status, stop, version, Rime schema management).
- **`typioc`**: Rejected. Too terse; `-ctl` suffix is a well-established Unix convention (`systemctl`, `loginctl`, `machinectl`).

## Consequences

- Positive: `typio` is freed as the project-level brand; no single component owns it.
- Positive: `typioctl` clearly signals "control tool" to Unix users.
- Positive: If the daemon is later extracted to its own repo, `typio-daemon` is available without conflict.
- Trade-off: Existing users and scripts that invoke `typio` must migrate to `typioctl`.
- Trade-off: Packagers must coordinate the binary rename in distribution packages.
