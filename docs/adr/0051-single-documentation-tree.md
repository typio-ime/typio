# ADR-0051: Single documentation tree at the repository root

- **Status**: Accepted
- **Date**: 2026-09-11
- **Deciders**: Core Maintainers

## Context

Crate directories carried their own documentation trees from the period when
`typioctl` and the framework core were separate repositories with independent
release cadences and contributor surfaces (archive ADR-0002):

- `crates/typio-runtime/docs/` — `adr/` (19 decisions), `dev/`, `explanation/`,
  `how-to/`, `reference/`, plus `index.md` and `engine-author.md`
- `crates/typio-control/docs/` — `adr/` (5 decisions), `dev/`, `how-to/`,
  `reference/`, plus `index.md`

After ADR-0039 and ADR-0045 folded these components into the workspace, the
duplicated trees became actively misleading:

- **Two routing systems.** `crates/typio-control/docs/dev/documentation-style-guide.md`
  and `crates/typio-runtime/docs/dev/contract-layers.md` each competed with the
  repository-level governance in `docs/dev/documentation/`.
- **Live content buried in a crate.** The engine-protocol reference, the engine
  contract, the configuration-system explanation, the engine-author walkthrough,
  and the voice-input explanation existed *only* under a crate, so the
  repository `docs/` index could not route readers to them.
- **Retired architecture presented as current.** `crates/typio-runtime/docs/adr/0002`
  declares "C ABI as the only public interface" — retired by ADR-0046 — while
  sitting next to live engine-contract documentation.
- **Dead names.** Both trees referenced `typio-core`, `typioctl`,
  `typio-wayland`, and `typio-vet`, none of which are crate names any more.

## Decision

One documentation tree, at the repository root, routed by the existing
three-gate governance (time → `docs/adr/`, audience → `docs/dev/`, cognitive
mode → `docs/how-to/` `docs/reference/` `docs/explanation/`).

- **Move live content into `docs/`.** `engine-contract.md`,
  `composition-state-machine.md`, `configuration-system.md`,
  `engine-host-resource-flow.md`, `modifier-key-consumption.md`,
  `architecture-overview.md`, and `voice-input.md` become
  `docs/explanation/`; `engine-protocol.md` becomes `docs/reference/`;
  the engine-author walkthrough becomes `docs/how-to/write-an-engine.md`; the
  CLI reference content merges into `docs/reference/cli.md`.
- **Preserve decisions as an archive, not as a parallel ADR set.**
  `crates/typio-runtime/docs/adr/` moves to
  `docs/adr/archive/framework-core/adr/` and `crates/typio-control/docs/adr/`
  to `docs/adr/archive/cli-control/`, each with its own `index.md` and a
  preserved original numbering. ADRs remain append-only; only their location
  changes. `docs/adr/index.md` points at both archives.
- **Delete the duplicated crate-local trees.** Their `dev/`, `how-to/`,
  `reference/`, and duplicate `index.md` pages are superseded by
  `docs/dev/`, `docs/how-to/`, `docs/reference/`, and `docs/index.md`.
  Everything a crate-local page still owns uniquely is moved, not dropped.
- **Keep cross-references relative and live.** Every reference to a moved page
  is rewritten to its new location in the same change.

## Alternatives considered

- **Keep crate-local trees and add a repository-level pointer to each.**
  Rejected: two routing systems with one authority is the problem, not the
  solution; a pointer does not stop the retired-architecture pages from reading
  as current.
- **Delete the historical ADRs.** Rejected: they are the record of *why*, and
  the ADR workflow treats them as immutable history. Archiving preserves the
  decision record while removing the parallel structure.
- **Renumber the archived ADRs into the workspace sequence.** Rejected: it
  breaks every existing cross-reference and destroys the provenance of
  decisions taken in a different repository.

## Consequences

- Positive: one documentation tree, one routing system, one governance entry
  point — the tree is navigable from `docs/index.md` alone.
- Positive: engine authors and protocol consumers can reach the engine
  reference and contract from the repository index.
- Positive: crate-local doc pages no longer assert retired architecture as
  current, and no page names a crate that does not exist.
- Trade-off: historical framework-core ADR numbers collide with workspace ADR
  numbers (both have an ADR-0001, ADR-0008, ADR-0018 …). Resolved by the
  `archive/` path prefix and per-archive indexes; prose that cites them must
  qualify which set it means.
