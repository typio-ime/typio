# Architecture Blueprints

Charter: this directory holds the **living state** of the system — how each
subsystem works *today*. Blueprints are HOT: they are updated in the same pull
request as the code they describe ([INV-TEMP-01](../governance/documentation/core/invariants.md)),
and they are the primary context for anyone working on a subsystem.

Division of labour:

- A **blueprint** states the current components, boundaries, and binding rules,
  and it may carry implementation coordinates.
- An **[explanation](../explanation/index.md)** page argues *why* the design is
  shaped that way and stays free of coordinates.
- An **[ADR](../adr/index.md)** is the immutable record of the decision, its
  rejected alternatives, and its trade-offs.

When a blueprint accumulates rules that no longer match code, that is a bug in
the blueprint, not a nuance: fix it in the same pull request.

| Blueprint | Scope | Founding decisions |
| :--- | :--- | :--- |
| [Panel Rendering](panel-rendering.md) | Panel content model, coordinator arbitration, scheduler, CPU canvas, SHM presentation | ADR-0005, 0014, 0017, 0022, 0023, 0040, 0044, 0050 |
| [Input Session](input-session.md) | Wayland input-method session, focus controller, text transactions, preedit coalescing | ADR-0002, 0003, 0018, 0042, 0043, 0047 |
| [Daemon Lifecycle](daemon-lifecycle.md) | systemd unit, the single poll loop, deadline folding, stall containment, latency budgets | ADR-0004, 0021, 0024, 0041, 0048, 0049 |
| [Engine Runtime](engine-runtime.md) | Engine discovery, manifests, worker processes, the Typio Engine Protocol, install layout | ADR-0025, 0029, 0030, 0046 |
| [Control Plane and Clients](control-and-clients.md) | TIP control surface, `typioctl`, `typio-settings`, tray surfaces, language switching | ADR-0008, 0026, 0031, 0033, 0034, 0045 |
| [Workspace Topology](workspace-topology.md) | Cargo workspace, crate responsibilities, dependency direction, Optics pinning, CI | ADR-0035, 0038, 0039, 0045, 0046, 0051 |

## Compaction status

No ADR has been compacted into a blueprint yet: each blueprint links to its
active records in [the ADR index](../adr/index.md) instead of tombstoning them.
The compaction triggers in [the Living Snapshot
entity](../governance/documentation/profiles/architecture/living-snapshot.md)
are met for the panel-rendering subsystem (more than five amending records), so
the next maintainer-led pass should tombstone that subsystem's superseded
records, relocate them to [docs/adr/archive/](../adr/archive/index.md), and
repoint the registry rows at [Panel Rendering](panel-rendering.md).

## See also

- [ADR Index](../adr/index.md) — immutable decisions, with a decision summary per record
- [Explanation](../explanation/index.md) — the reasoning behind these designs
- [Module Map](../dev/module-map.md) — crate and module coordinates
