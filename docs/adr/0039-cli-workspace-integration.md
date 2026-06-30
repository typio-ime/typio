# ADR-0039: CLI Workspace Integration

- **Status**: Accepted
- **Date**: 2026-06-30
- **Deciders**: Typio maintainers
- **Supersedes**: `crates/typioctl/docs/adr/0002-independent-cli-repository.md`
- **Amends**: ADR-0038

## Context

ADR-0038 moved `libtypio`, `typio-abi`, and `typio-vet` into the main Typio
workspace so framework, ABI, vet, and host changes can land atomically.

`typioctl` has the same coupling pressure from the other side of the daemon:
it is the canonical TIP/UDS client, and every daemon protocol rename, method
addition, error-shape change, or event-shape change must be reflected in the
CLI. Keeping the CLI in a sibling repository made those protocol changes span
multiple commits and repositories, even though the CLI has no independent
runtime dependency stack.

`typio-settings` has a different shape. It is a Meson/C application with GTK,
glib/gio, desktop-file, icon, appstream, and translation concerns. It benefits
from sharing protocol references, but it should not become part of the Cargo
workspace or make the daemon's normal Rust build depend on the settings-panel
toolchain.

Engine repositories remain independent. They are runtime plugins/workers with
different upstream dependency stacks and release cadence.

## Decision

Move `typioctl` into the main Typio repository as `crates/typioctl` and make it
a Cargo workspace member.

The intended local checkout layout is:

```text
~/projects/typio/           # main product workspace
~/projects/typio-engines/   # engine repositories
~/projects/typio-settings/  # settings panel repository
```

The main workspace owns:

- `crates/typio-host` — Wayland daemon binary (`typio`)
- `crates/libtypio` — platform-neutral framework
- `crates/typio-abi` — shared ABI types
- `crates/typio-vet` — native engine ABI vetting
- `crates/typioctl` — command-line TIP/UDS client

For now, `typioctl` keeps its local `ipc.rs` implementation. A later change may
extract shared daemon/client framing and method constants into a `typio-ipc`
crate once the duplication is being actively changed.

## Alternatives Considered

- **Keep `typioctl` as a sibling repository.** Rejected because the CLI is a
  protocol client for this daemon. Cross-repository protocol changes add
  coordination cost without buying meaningful release independence.
- **Merge `typio-settings` into the Cargo workspace at the same time.**
  Rejected because it is not a Cargo crate and carries a separate desktop-app
  build stack. It can remain a sibling checkout without blocking atomic daemon
  and CLI protocol changes.
- **Move all engines into the main repository.** Rejected. Engines have
  independent runtime dependencies, packaging, and release cadence.
- **Extract `typio-ipc` before moving `typioctl`.** Deferred. Moving first
  makes the protocol drift visible in one workspace; extracting the shared
  crate is then a focused refactor rather than a prerequisite.

## Consequences

- Positive: daemon and CLI protocol changes can land in one commit.
- Positive: `cargo check -p typioctl` and `cargo test -p typioctl` run from the
  same root as host tests.
- Positive: CLI docs can link directly to the daemon IPC reference.
- Trade-off: the main repository is no longer only the host/framework runtime;
  it also contains the canonical command-line client.
- Trade-off: `typioctl` historical ADRs remain under `crates/typioctl/docs/adr`
  as imported records, while new repository-layout decisions live in the root
  ADR set.
