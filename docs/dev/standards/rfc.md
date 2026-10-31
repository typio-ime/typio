# Request for Comments (RFC) Standard

This document defines the deliberation protocol for cross-cutting, high-reversal-cost subsystem redesigns in Typio prior to ADR ratification.

---

## 1. When to Author an RFC

An RFC is reserved for substantial architectural proposals where multiple viable designs exist and cross-subsystem consensus is required before implementation:

- Introducing a new crate or major protocol version (e.g. TIP v2, new IPC modality).
- Redesigning the event loop or threading model.
- Restructuring public CLI or configuration schemas.

Typio primarily operates an ADR-first workflow. An RFC is optional and serves as the staging ground for complex deliberations before drafting the binding ADR.

---

## 2. RFC Lifecycle

```text
[ Proposal / Draft ] ──► [ Deliberation ] ──► [ Consensus Reached ] ──► [ ADR Ratified ]
                              │
                              └──► [ Withdrawn / Deferred ]
```

1. **Proposal**: Author creates an RFC document outlining problem space, design options, and migration strategy.
2. **Deliberation**: Architecture working group evaluates prototypes, performance measurements, and invariant impacts.
3. **Ratification**: The chosen alternative is codified into an authoritative ADR in `docs/adr/`.
4. **Archive**: Upon conclusion, the RFC is archived or linked from the resulting ADR.

---

## 3. RFC Document Structure

1. **Summary**: Concise one-paragraph elevator pitch.
2. **Motivation**: Concrete user or system pain points motivating the change.
3. **Detailed Design**: Architecture, protocol frames, API sketches, error states.
4. **Drawbacks**: Costs, dependencies, migration friction.
5. **Alternatives Considered**: Direct exploration of rejected design variants.
6. **Unresolved Questions**: Open items to settle during prototyping.
