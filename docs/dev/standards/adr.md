# Architectural Decision Records (ADR) Standard

This document establishes the qualification criteria, lifecycle state machine, schema, and authoring rules for Architectural Decision Records in Typio.

---

## 1. Purpose of an ADR

An Architectural Decision Record captures a significant architectural choice, its context, its trade-offs, and why alternative approaches were rejected. ADRs represent the immutable epistemic lineage of Typio's architectural evolution.

---

## 2. The 3-Question Qualification Test

A change warrants an ADR if and only if it answers **YES** to at least one of these questions:

1. **Cross-Boundary Blast Radius**: Does the decision alter wire contracts, process boundaries, or IPC schemas between crates (e.g. Engine Protocol, TIP socket, Wayland FFI)?
2. **Invariant Alteration**: Does it mint, amend, supersede, or retire a canonical System Invariant (`[INV-*]`)?
3. **High Reversal Cost**: Would reverting this technical decision require substantial multi-crate refactoring or break external compatibility?

Routine bug fixes, internal refactorings within a single crate, and dependency patch updates do not qualify.

---

## 3. In-Place Lifecycle States

ADRs follow an immutable, in-place lifecycle. A ratified decision is never edited to change historical reality; subsequent shifts author a superseding record:

```text
[ Draft ] ──► [ Accepted ] ──► [ Superseded by ADR-NNNN ]
    │
    └──► [ Rejected ]
```

- **`draft`**: Under active deliberation.
- **`accepted`**: Ratified and actively binding on the codebase.
- **`superseded`**: Replaced by a subsequent decision (`superseded_by: ADR-NNNN`). Historical context and negative knowledge remain permanently binding as negative constraints.
- **`rejected`**: Deliberated and rejected. Preserves negative knowledge to prevent re-opening dead ends.
- **`deprecated`**: Subsystem retired without a direct successor.

---

## 4. Frontmatter Schema

Every ADR file in `docs/adr/NNNN-slug.md` begins with YAML frontmatter:

```yaml
---
id: ADR-NNNN
title: "Decision Title in Title Case"
status: accepted # draft | accepted | superseded | rejected | deprecated
date: YYYY-MM-DD
superseded_by: null # or "ADR-MMMM"
scope: loop | panel | engine | ipc | architecture | performance
negative_knowledge: true
---
```

---

## 5. Mandatory Negative Knowledge Section

Every ADR **must** include a `## Rejected Alternatives & Negative Knowledge` section. Explaining why discarded options were rejected is critical for preventing future developers and AI agents from resurrecting failed patterns ([INV-AGENT-01]).

---

## 6. ADR Template

```markdown
# ADR-NNNN: [Title of Decision]

- **Status**: [Draft | Accepted | Superseded by ADR-MMMM | Rejected]
- **Date**: YYYY-MM-DD
- **Scope**: [loop | panel | engine | ipc | architecture | performance]
- **Deciders**: [Names / Handles]

## Context

[Describe the forces, technical constraints, and current state driving this decision.]

## Decision

[State clearly and unambiguously what architectural choice is being made.]

## Rationale

[Detail why this choice satisfies the constraints and what trade-offs were accepted.]

## Invariants & Behavioral Boundaries

- `[INV-SUB-XX]`: [State any binding system invariant introduced or modified.]

## Rejected Alternatives & Negative Knowledge

- **[Alternative A]**: Rejected because [concrete technical reason / failure mode].
- **[Alternative B]**: Rejected because [concrete technical reason / failure mode].

## Consequences

- **Positive**: [Expected architectural benefits.]
- **Negative & Trade-offs**: [Incurred complexity or maintenance burden.]
```
