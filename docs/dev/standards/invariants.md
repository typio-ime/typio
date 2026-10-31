# Invariant & Constraint Governance

This document codifies the definition, categorization, and lifecycle of invariants in Typio. It establishes an uncompromising policy against pseudo-invariant tag proliferation, ensuring that invariants remain high-value architectural anchors rather than ceremonial comments.

---

## 1. Core Principle: Mechanisms Over Exhortations

In software engineering, an invariant is a condition that must unconditionally hold across all valid states of a system. Historically, projects suffer from "invariant inflation" when mundane UI styles, layout heuristics, or ephemeral flags are labeled as invariants without verification mechanics.

**The Golden Rule**:
> **If a rule has neither a compiler-level proof nor an automated test assertion, it is NOT an invariant — it is merely an aspiration.**

---

## 2. Constraint Admissibility Matrix

Every architectural and operational rule maps deterministically into one of four categories:

| Category | Definition & Scope | Identifier Policy | Mandatory Enforcement Mechanism |
| :--- | :--- | :--- | :--- |
| **System Invariant** | Non-negotiable structural, lifecycle, and rendering red-lines with cross-boundary blast radius (process isolation, reactor cadence, memory bounds). Max ~20 total repository-wide. | Formally ratified in ADRs (`[INV-PANEL-*]`, `[INV-LOOP-*]`, `[INV-WKS-*]`) | 1. Package dependency firewalls<br>2. Dedicated regression test suite (`test_inv_*` or integration tests)<br>3. Compiler typestates & RAII guards |
| **Domain Contract** | State-machine transitions and behavioral rules within a single module or subsystem (e.g. `CompositionState`, `EngineWorkerState`). | **NO global `INV-` tag**.<br>Expressed as named types. | 1. Typed `enum` variants & private field encapsulation<br>2. Unit tests and property tests |
| **Design Specification** | UI styling, candidate margin tokens, default font size, keyboard shortcut mappings. | **NO `INV-` tag**.<br>Expressed as design constants. | 1. Config schemas and platform defaults<br>2. UI layout tests |
| **Implementation Note** | Local performance workarounds, compiler optimizations, kernel flag workarounds. | **NO `INV-` tag**.<br>Standard code comments. | Direct regression unit test |

---

## 3. The 3-Condition Gate for System Invariants

A rule may be elevated to a **System Invariant** if and only if it satisfies all three criteria:

1. **High Blast Radius**: Violation results in input stall, daemon crash, compositor dead-lock, or corrupted preedit state.
2. **Durable Architecture**: It represents a foundational architectural pillar that will not become obsolete due to routine feature iterations.
3. **Mechanically Enforced**: It specifies an automated enforcement mechanism (compiler type check, dependency rule, kernel isolation flag, or deterministic automated test).

---

## 4. Canonical System Invariants Registry

The following authoritative system invariants are binding across the repository:

### Workspace & Topology Invariants
- **`[INV-WKS-01]`**: One virtual workspace root. Member packages are declared only in the root manifest ([ADR-0038](../../adr/0038-framework-abi-vet-monorepo.md)).
- **`[INV-WKS-02]`**: Dependency direction is strictly concentric. Lower layers never depend on higher layers ([ADR-0047](../../adr/0047-headless-platform-state-decoupling.md)).
- **`[INV-WKS-03]`**: Recognition engines are reached only through Typio Engine Protocol over child process socketpairs ([ADR-0054](../../adr/0054-engine-protocol-only-no-in-process-abi.md)).
- **`[INV-WKS-04]`**: External clients communicate with the daemon exclusively via TIP JSON-RPC over the Unix domain socket ([ADR-0008](../../adr/0008-ipc-protocol-resource-namespaces-uds-only.md)).
- **`[INV-WKS-05]`**: `typio-runtime` is embedded and headless, independent of Wayland or graphics libraries.
- **`[INV-WKS-06]`**: Optics Rust bindings resolve from tagged Git releases in canonical builds, with local overrides restricted to development worktrees.
- **`[INV-WKS-08]`**: `typio-daemon` is the single version source for the product suite.

### Panel Rendering Invariants
- **`[INV-PANEL-08]`**: A free SHM buffer is reserved from the pool before any render pass begins ([ADR-0044](../../adr/0044-bounded-panel-rendering.md)).
- **`[INV-PANEL-12]`**: Dropped or skipped frames never alter logical input or selection state ([ADR-0044](../../adr/0044-bounded-panel-rendering.md)).
- **`[INV-PANEL-13]`**: No GPU context, swapchain, or texture exists in the Panel path; rendering is 100% CPU canvas to `wl_shm` ([ADR-0040](../../adr/0040-cpu-canvas-render-shm-buffers.md), [ADR-0053](../../adr/0053-optics-v0.0.62-adoption-boundary.md)).
- **`[INV-PANEL-14]`**: Every Panel pass explicitly clears its framebuffer to transparent before drawing geometry ([ADR-0053](../../adr/0053-optics-v0.0.62-adoption-boundary.md)).

### Reactor Cadence Invariants
- **`[INV-LOOP-01]`**: Single reactor thread driving Wayland, IPC, and engine sockets via `epoll` ([ADR-0004](../../adr/0004-event-loop-scheduling-and-watchdog.md)).
- **`[INV-LOOP-03]`**: No periodic tick. The poll timeout is unbounded by default and only armed when explicit deadlines exist ([ADR-0024](../../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md)).
- **`[INV-LOOP-09]`**: Engine respawn attempts are exponentially backoff-limited to prevent crash loops.

### Documentation Governance Invariants
- **`[INV-CORE-01]` Audience Firewall**: Public documentation (`docs/{tutorials,how-to,reference,explanation}/`) must NEVER link into internal contributor docs (`docs/dev/**`).
- **`[INV-CORE-02]` Root Whitelist**: Root markdown files are strictly limited to `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `AGENTS.md`, `SECURITY.md`, and `LICENSE`.
- **`[INV-CORE-04]` Conceptual Explanation**: `docs/explanation/` is conceptual prose; implementation coordinates, code fences, and symbols belong in `docs/reference/` or `docs/dev/`.
- **`[INV-CORE-05]` Directory Charter**: Every documentation directory must contain an `index.md` or `README.md` stating its charter.

---

## 5. Degazetting & Retirement Policy

1. **Pseudo-Invariants**:
   - Ad-hoc or localized tags outside the canonical registry are prohibited.
   - Code comments should use standard Rust doc-comments or domain types rather than inventing pseudo-invariants.
2. **True Invariants**:
   - When a system invariant evolves or is superseded, it follows the **In-Place Lifecycle**:
     - `Active`: Bound and verified in current releases.
     - `Superseded`: Formally superseded by a successor invariant via an ADR (`superseded_by: ADR-NNNN`), retaining documented failure modes.
     - `Deprecated`: Retired due to subsystem removal, with historical context preserved in the ADR registry.
