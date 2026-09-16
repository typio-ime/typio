# Repository Contracts

Reference data, profile declarations, and path bindings for documentation surfaces adopted by this repository.

For adoption procedures, see [Workflow: Adoption](core/workflow.md#part-4-repository-adoption-workflow).

---

## 1. Activated Profiles

Declare the domain capability profiles active in this repository. `tools/sync.sh` and `tools/verify.sh` use this declaration to assemble and verify documentation surfaces.

- [x] `core` (Mandatory: 4D spatial taxonomy, system invariants, operational workflow, style)
- [x] `architecture` (Architecture records, living blueprints, pre-decision RFCs)
- [x] `validation` (Product validation: user journeys, acceptance matrices, testing guides)
- [ ] `operations` (Operational knowledge: postmortems, triage runbooks)

Profile decisions for this repository:

- **`architecture` is active.** Typio maintains a live ADR log in `docs/adr/` and living subsystem blueprints in `docs/architecture/`.
- **`validation` is active.** Typio ships a user-visible product with a real user journey (`docs/dev/acceptance.md`) and a Cargo test workspace (`docs/dev/testing.md`).
- **`operations` is inactive.** There is no incident-response or on-call rotation for this project. Activate it only when a real production outage review exists to record. Triage procedures currently live in [How-to: Troubleshooting](../../how-to/troubleshooting.md).
- **The RFC workflow is not adopted.** Per the Golden Rule in the [RFC entity](profiles/architecture/rfc.md), a project without a multi-stakeholder review group must not introduce RFCs: proposals go directly to a `Proposed` ADR. The `docs/rfc/` surface is deliberately absent.

---

## 2. Directory Layout Bindings

| Surface | Path | Required | Temperature | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| **Core Governance** | `docs/governance/documentation/` | Yes | **HOT** | Mirrored governance standard (`core/` + active profiles) |
| **Project Governance** | `docs/governance/` | Yes | **HOT** | Repository charter, change-control gates, and release process |
| **Contributor Firewall** | `docs/dev/` | Yes | **HOT** | Developer bootstrap, module map, testing, and acceptance |
| **Active ADRs** | `docs/adr/` | Yes (`architecture`) | **WARM** | Immutable Architectural Decision Records |
| **Living Blueprints** | `docs/architecture/` | Yes (`architecture`) | **HOT** | Living subsystem blueprints and compacted invariants |
| **Archived ADRs** | `docs/adr/archive/` | Yes (`architecture`) | **COLD** | Compacted, superseded, and retired records |
| **In-Flight RFCs** | — | Not adopted | — | Proposals go directly to a `Proposed` ADR |
| **Archived RFCs** | — | Not adopted | — | No RFC workflow exists to archive |
| **Incident Reviews** | — | Not adopted (`operations` inactive) | — | Activate `operations` before recording postmortems |
| **Root Entry** | `README.md` | Yes | **HOT** | Project value pitch and shortest setup |
| **Docs Portal** | `docs/index.md` | Yes | **HOT** | Primary documentation navigation portal |

---

## 3. Optional Document Contracts

| Contract Surface | Active Profile | If Present | If Absent |
| :--- | :--- | :--- | :--- |
| `CHANGELOG.md` | Universal | Present. User-visible changes update it in the same PR | Omit changelog checks from PR review |
| `CONTRIBUTING.md` | Universal | Present. Contributor entry point linking into `docs/dev/` | Add before accepting outside contributions |
| `docs/dev/setup.md` | Universal | Present. Cold-start build and dev-loop instructions | Contributor firewall is incomplete |
| `docs/dev/acceptance.md` | Profile `validation` | Present. User-visible feature changes update it in the same PR | Rely on internal testing guide only |
| `docs/dev/testing.md` | Profile `validation` | Present. Test runner, suite, or CI changes update it in the same PR | Document testing in the dev setup guide |
| `docs/dev/module-map.md` | Universal | Present. Holds implementation coordinates (source maps, symbols) that `docs/explanation/` must not carry | Keep coordinates in inline code comments only |
| `docs/adr/index.md` | Profile `architecture` | Present. Active and archived ADRs registered in one table | Create the index before authoring ADRs |
| `docs/reference/glossary.md` | Universal | Present. Canonical project terms defined and cross-linked | Keep term definitions local to documents |

---

## 4. Typio-Specific (Tier 3) Governance

Rules that must never enter the portable standard, because they are specific to this repository, live in [Repository Governance](../index.md):

- Rust edition, formatting, and clippy policy: [Developer Code Style](../../dev/code-style.md).
- Commit, tag, versioning, and release process: [Repository Governance](../index.md#release-process).
- Cross-repository development against the sibling `optics` checkout: [Optics Dev Worktree](../../dev/optics-dev-worktree.md).
- Interface stability tiers: [Interface Stability](../../reference/stability.md).

---

## 5. Mirror Provenance and Refresh

| Field | Value |
| :--- | :--- |
| Standard | `docs-governance` |
| Protocol version | `5.1.0` (schema 2) |
| Source checkout | `../docs-governance` |
| Mirror root | `docs/governance/documentation/` |

Refresh procedure (run from the `docs-governance` checkout):

```bash
./tools/sync.sh <typio checkout>
./tools/verify.sh <typio checkout>
```

Only `contracts.md` is editable locally; every other file in this directory is
hash-verified against `.manifest.json`. Re-run `tools/check-docs.sh` in the
Typio checkout after a refresh.
