# Developer & Contributor Documentation

Charter: this directory is the **contributor plane**. Pages here document how to
build, test, change, and release Typio, and they may contain implementation
coordinates — source paths, module names, and ownership tables — that the
user-facing planes must not carry. Nothing here is reachable from
[docs/how-to/](../how-to/index.md), [docs/reference/](../reference/index.md), or
[docs/explanation/](../explanation/index.md): the contributor firewall runs one
way ([INV-CORE-01](../governance/documentation/core/invariants.md)).

| Page | Covers |
| :--- | :--- |
| [Developer Setup](setup.md) | Native prerequisites, the sibling `optics`/`flux` build, workspace builds, running the daemon locally |
| [Module Map](module-map.md) | Crates, modules, and ownership — where implementation coordinates live |
| [Testing](testing.md) | Workspace test suites, per-crate coverage, environment matrix, CI gates |
| [Acceptance](acceptance.md) | Real user journeys from a cold start, and the acceptance scenario matrix |
| [Code Style](code-style.md) | Language versions, formatting, doc comments, and design preferences |
| [Panel Appearance](panel-appearance.md) | Panel render pipeline, fonts, theme resolution, cache invalidation |
| [Optics Dev Worktree](optics-dev-worktree.md) | Developing against a live Optics worktree and promoting an Optics release |

## Related

- [Repository Governance](../governance/index.md) — commit, tag, release, and review charters
- [Architecture Blueprints](../architecture/index.md) — the current state of each subsystem
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — the contribution entry point
