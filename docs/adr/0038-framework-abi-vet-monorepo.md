# ADR-0038: Framework, ABI, and Vet Monorepo

- **Status**: Accepted
- **Date**: 2026-06-30
- **Deciders**: Typio maintainers
- **Supersedes**: ADR-0035 decision D1 for repository layout

## Context

The Rust host, framework core, C ABI types, and engine conformance tooling have
become one change surface. Recent work on language switching, dynamic engine
capabilities, voice sessions, host-managed candidate selection, and focus
lifecycle behavior crosses `typio-host`, `libtypio`, `typio-abi`, and
`typio-vet`.

Keeping `libtypio` in a sibling repository made these changes harder than the
architecture required. A contract change needed one commit in `libtypio`, a
tag or path override, then another commit in `typio-linux`. `typio-vet` could
lag the ABI it was supposed to validate. Documentation and changelog entries
were split across repositories even when the behavior was user-visible only
through the Linux host.

ADR-0035 deliberately kept `libtypio` independent during the bilingual Rust
migration to reduce risk while the host was still being ported. That migration
has now crossed the point where atomic framework/host changes are more valuable
than repository separation.

## Decision

Move the framework and ABI tooling into this Cargo workspace:

- `crates/libtypio` contains the framework core and the `libtypio` package.
- `crates/typio-abi` contains the shared Rust representation of the C ABI.
- `crates/typio-vet` contains engine conformance tooling.
- `crates/typio-host` continues to contain the Wayland daemon.

The first step is a move, not a semantic split. The `libtypio` package name,
library name, public C headers, and public Rust modules remain intact. The host
uses workspace path dependencies for `libtypio` and `typio-abi`; no git tag or
sibling checkout is needed for local builds.

The historical framework documentation and ADR set move with the crate under
`crates/libtypio/docs/`. New decisions that affect the combined repository
layout, host/framework integration, or release process belong in the root
`docs/adr/` set.

## Alternatives considered

- **Keep `libtypio` as a sibling git dependency**: Rejected because ABI,
  framework, vet, and host changes now routinely need to land atomically.
- **Move and simultaneously rename/split `libtypio` into `typio-core` and
  `typio-abi`**: Rejected for this step. Renaming the framework crate and
  splitting modules is a real API design task; doing it during the repository
  move would obscure build and behavior regressions.
- **Promote every sibling Typio project into one repository immediately**:
  Rejected for now. Engine packages can still validate the public contract via
  `typio-vet`, and keeping them external tests the ABI boundary.

## Consequences

- Positive: Framework, ABI, vet, and host changes can be reviewed, tested, and
  released atomically.
- Positive: `typio-vet` can track ABI changes in the same commit that changes
  the ABI.
- Positive: A fresh host checkout contains the Rust framework dependency
  graph; only native renderer dependencies remain external.
- Trade-off: The repository becomes broader than a Linux-only host. Project
  documentation must distinguish platform host code from framework code by
  crate path rather than repository path.
- Negative (accepted): External engine repositories that previously pointed at
  the standalone `libtypio` repository need their development dependency
  examples updated to point at this workspace or at a future published package.
