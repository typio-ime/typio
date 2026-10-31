# Agent Directives

This document is the entry point for AI agents working in this repository. It codifies operational constraints, architectural red-lines, and authoritative standards routing.

---

## 1. Operational Guardrails (Hard Constraints)

- **Prohibited Git Commands**: NEVER execute `git stash`, `git branch`, or `git reset`.
- **Command Termination**: All shell commands MUST be finite and self-terminating. Never launch background daemons or unbounded polling loops without a strict timeout.
- **Clean Git State**: Do not leave scratch files, temporary binaries, or untracked test logs in the working tree.

---

## 2. Core Mental Model & Subsystem Boundaries

- **Workspace Topology**: 11 member crates layered concentrically (`typio-host-types` / `typio-engine-protocol` / `typio-engine-manifest` → `typio-runtime` → `typio-host-platform` / `typio-client` → `typio-daemon` / `typio-settings` / `typio-control` / `typio-engine-check` / `xtask`). Dependencies flow strictly inward.
- **Engine Process Isolation (`[INV-WKS-03]`)**: Recognition engines are independent child processes communicating over a private `socketpair` via typed binary `Engine Protocol` frames. No in-process dynamic library (`dlopen`) C ABI exists ([ADR-0054](docs/adr/0054-engine-protocol-only-no-in-process-abi.md)).
- **TIP Control Surface (`[INV-WKS-04]`)**: All external control, state queries, and configuration updates flow over `/run/user/<uid>/typio/tip.sock` using JSON-RPC TIP v1 ([ADR-0008](docs/adr/0008-ipc-protocol-resource-namespaces-uds-only.md)).
- **Single-Thread Reactor Purity (`[INV-LOOP-01]`, `[INV-LOOP-03]`)**: The event loop runs on a single main thread driving `epoll` across Wayland, IPC, engine child pipes, and one-shot timerfd timers. No periodic baseline tick exists; poll timeouts are unbounded unless a real deadline is pending.
- **CPU Canvas & SHM Rendering (`[INV-PANEL-13]`, `[INV-PANEL-14]`)**: The candidate popup renders via `flux_canvas_create_cpu` onto `wl_shm` pool buffers. Zero GPU objects exist in the Panel path. Every Panel pass clears its framebuffer to transparent before drawing geometry ([ADR-0044](docs/adr/0044-bounded-panel-rendering.md), [ADR-0053](docs/adr/0053-optics-v0.0.62-adoption-boundary.md)).
- **Companion Optics Dev Worktree**: Cross-repo co-development with `../optics` uses a linked development worktree (`../typio-dev/`) with `.cargo/config.toml` path patches. Canonical builds on `main` pin release tags in `Cargo.toml`.

---

## 3. Engineering Discipline

- **Mechanisms Over Exhortations**: Rely on Rust compiler typestates, private visibility, RAII guards, and automated tests to guarantee invariants. Never rely on natural-language comments alone.
- **Anti-Inflation**: Do not mint pseudo-invariant tags. Adhere to the canonical registry (`[INV-PANEL-*]`, `[INV-LOOP-*]`, `[INV-WKS-*]`, `[INV-CORE-*]`). State transitions belong in named Rust enums.
- **Audience Firewall (`[INV-CORE-01]`)**: Public documentation (`docs/{tutorials,how-to,reference,explanation}/`) must NEVER link into internal contributor docs (`docs/dev/**`).
- **Atomic Verification**: Always verify code changes with the mandatory local gates before declaring tasks complete.

---

## 4. Mandatory Verification Gates

Run these gates before concluding any work:

```bash
tools/check-docs.sh                       # documentation invariants and link integrity
cargo fmt -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

---

## 5. On-Demand Standards Routing

Do not guess repository rules or rely on stale assumptions. Consult authoritative specifications as needed:

- **Standards Suite Catalog**: [`docs/dev/standards/README.md`](docs/dev/standards/README.md)
  - System Architecture & Red-Lines: [`docs/dev/standards/architecture.md`](docs/dev/standards/architecture.md)
  - Invariant & Constraint Policy: [`docs/dev/standards/invariants.md`](docs/dev/standards/invariants.md)
  - ADR Authoring & Lifecycle: [`docs/dev/standards/adr.md`](docs/dev/standards/adr.md)
  - RFC Standards: [`docs/dev/standards/rfc.md`](docs/dev/standards/rfc.md)
  - Documentation Taxonomy & Firewall: [`docs/dev/standards/taxonomy.md`](docs/dev/standards/taxonomy.md)
  - Markdown & Hyperlink Style: [`docs/dev/standards/style.md`](docs/dev/standards/style.md)
  - Engineering Workflow & Gates: [`docs/dev/standards/workflow.md`](docs/dev/standards/workflow.md)
  - Blameless Incident Postmortems: [`docs/dev/standards/postmortems.md`](docs/dev/standards/postmortems.md)
- Architectural Blueprints: [`docs/architecture/index.md`](docs/architecture/index.md)
- Architectural Decision Records: [`docs/adr/index.md`](docs/adr/index.md)
- Developer Setup: [`docs/dev/setup.md`](docs/dev/setup.md)
- Optics Dev Worktree Workflow: [`docs/dev/optics-dev-worktree.md`](docs/dev/optics-dev-worktree.md)
- Source Module Map: [`docs/dev/module-map.md`](docs/dev/module-map.md)
- Testing & Acceptance: [`docs/dev/testing.md`](docs/dev/testing.md), [`docs/dev/acceptance.md`](docs/dev/acceptance.md)
