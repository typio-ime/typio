# Architecture Decision Records

ADRs are append-only records of significant design decisions in `libtypio` — the platform-neutral core library. Once accepted, they are not edited; a new ADR supersedes an old one if a decision changes.

Decisions that only apply to downstream components (the Wayland host, the CLI, the settings panel, individual engines) live in those repositories' own ADR sets.

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-0001](0001-record-architecture-decisions.md) | Record Architecture Decisions | Accepted |
| [ADR-0002](0002-c-abi-as-the-only-public-interface.md) | C ABI as the only public interface | Accepted |
| [ADR-0003](0003-plugin-engine-abi-dual-category.md) | Plugin engine ABI with dual-category (keyboard/voice) slots | Accepted |
| [ADR-0004](0004-platform-neutral-core-host-loading.md) | Platform-neutral core, host-owned plugin loading, out-of-tree engines | Accepted |
| [ADR-0005](0005-internal-engine-backend-abstraction.md) | Internal engine backend abstraction | Accepted |
| [ADR-0006](0006-composition-state-and-commit-event.md) | Composition as state, commit as event | Accepted |
| [ADR-0007](0007-ipc-ownership-host-and-engine-backend-deferred.md) | IPC ownership — control surface to host, engine backend deferred | Accepted |
| [ADR-0008](0008-engine-properties-unified-into-config-schema.md) | Engine properties unified into the config schema layer | Accepted |
| [ADR-0009](0009-engine-status-reflection-engagement-and-active-profile.md) | Engine status reflection — engagement axis + open active profile | Superseded by ADR-0011 |
| [ADR-0010](0010-keyboard-status-domain-and-salience.md) | Keyboard status is keyboard-domain; announcement salience | Superseded by ADR-0011 |
| [ADR-0011](0011-engine-mode-as-first-class-concept.md) | Engine Mode as a first-class framework concept | Accepted |
| [ADR-0012](0012-host-managed-candidate-selection.md) | Host-managed candidate selection | Superseded by ADR-0013 |
| [ADR-0013](0013-host-managed-selection-v2.md) | Host-managed candidate selection — amended flags | Accepted |
| [ADR-0014](0014-engine-availability-axis.md) | Engine availability as a first-class lifecycle axis | Accepted |
| [ADR-0015](0015-ipc-only-engine-backend.md) | IPC-Only Engine Backend | Superseded by ADR-0017 |
| [ADR-0016](0016-out-of-process-active-mode-reflection.md) | Out-of-process active-mode reflection | Accepted (terminology amended by ADR-0017) |
| [ADR-0017](0017-typio-engine-protocol.md) | Typio Engine Protocol and engine-process registration | Accepted |
| [ADR-0018](0018-language-first-switching.md) | Language-first switching — language as the user-facing switch unit | Accepted |

## Looking for something else?

- Current design docs: [Explanation](../explanation/)
- Developer docs: [dev/](../dev/)
- Host-side ADRs: see the root `docs/adr/` set
- CLI ADRs: see `../../typioctl/docs/adr/`
- Settings-panel ADRs: see the `typio-settings` repository
