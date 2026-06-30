# ADR-0001: Record Architecture Decisions

- **Status**: Accepted
- **Date**: 2026-05-28
- **Deciders**: Project maintainers

## Context

`libtypio` is the platform-neutral core of the Typio input method: a Rust crate with a hand-curated public C ABI consumed by host binaries (`typio-wayland`), plugin engines (`typio-engine-rime`, `typio-engine-mozc`, …), and any other downstream that links it. As the project grows, design choices about the ABI shape, engine model, and composition contract accumulate. Without explicit records, the reasoning behind those choices is lost and must be rediscovered.

## Decision

This project uses Architecture Decision Records (ADRs) stored in `docs/adr/`.

- Each ADR is numbered sequentially and is append-only after acceptance.
- To change a past decision, write a new ADR that supersedes the old one and update the old ADR's status field only.
- ADRs are short (ideally one page) and focus on context, decision, alternatives, and consequences.
- ADRs that no longer apply to this repository (e.g. host- or CLI-specific decisions after a split) are moved to the downstream repository that now owns them, not deleted.

## Alternatives considered

- **Inline design comments in code**: Rejected. Comments describe *what* the code does, not *why* a larger design choice was made.
- **Long-form architecture documents only**: Rejected. Explanation docs are mutable; ADRs provide an immutable anchor for specific decisions.

## Consequences

- Positive: new contributors understand why key boundaries exist without reading the entire commit history.
- Positive: reviewers can require an ADR for architectural changes, creating a lightweight gate.
- Trade-off: maintainers must remember to write ADRs for significant decisions.
