# System Architecture Governance

This document codifies the workspace topology, subsystem boundaries, and non-negotiable architectural red-lines of Typio.

---

## 1. Workspace Topology and Crate Layers

Typio organizes its Cargo workspace into concentric layers ([ADR-0038](../../adr/0038-framework-abi-vet-monorepo.md), [ADR-0047](../../adr/0047-headless-platform-state-decoupling.md)). Dependencies flow strictly from outer layers inward; backward or circular dependencies across layer boundaries are prohibited:

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│ Layer 4: Executables & Entrypoints                                             │
│   ├── crates/typio-daemon        (Wayland input-method daemon host)            │
│   ├── crates/typio-settings      (Optics Iris graphical settings application)  │
│   ├── crates/typio-control       (typioctl CLI client for TIP socket)          │
│   ├── crates/typio-engine-check  (Black-box engine conformance probe tool)    │
│   └── crates/xtask               (Workspace build & release automation)        │
└──────────────────────────────────────┬─────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼─────────────────────────────────────────┐
│ Layer 3: Platform Integration & Control Surface                                │
│   ├── crates/typio-host-platform (Wayland FFI, Flux CPU canvas, SHM pool)      │
│   └── crates/typio-client        (Shared TIP Unix domain socket client)        │
└──────────────────────────────────────┬─────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼─────────────────────────────────────────┐
│ Layer 2: State Machine & Execution Runtime                                     │
│   └── crates/typio-runtime       (Headless input-method state machine, engine  │
│                                   scheduler, process supervisor)               │
└──────────────────────────────────────┬─────────────────────────────────────────┘
                                       │
┌──────────────────────────────────────▼─────────────────────────────────────────┐
│ Layer 1: Contracts & Neutral Types                                             │
│   ├── crates/typio-engine-protocol (Typed wire protocol frames over socketpair)│
│   ├── crates/typio-engine-manifest (Engine manifest TOML contract & discovery) │
│   └── crates/typio-host-types      (Platform-neutral UI state & configuration) │
└────────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Foundational Architectural Red-Lines

### 2.1 Engine Process Isolation
- **Red-Line**: Recognition engines must NEVER run in-process or link into the daemon via dynamic library (`dlopen`) C ABIs ([ADR-0054](../../adr/0054-engine-protocol-only-no-in-process-abi.md)).
- **Enforcement**: Every engine is an independent child process spawned by the daemon supervisor. Communication occurs strictly over a private `socketpair` using typed length-prefixed `Engine Protocol` binary frames (`[INV-WKS-03]`). Crashing or slow engines are isolated from the main input thread.

### 2.2 TIP UDS IPC Control Surface
- **Red-Line**: External clients (CLI, settings, desktop extensions) must NEVER bypass the TIP Unix domain socket or mutate daemon state directly.
- **Enforcement**: All management, configuration, and state queries flow over the `/run/user/<uid>/typio/tip.sock` endpoint using the JSON-RPC TIP v1 protocol ([ADR-0008](../../adr/0008-ipc-protocol-resource-namespaces-uds-only.md), `[INV-WKS-04]`).

### 2.3 Single-Thread Reactor Purity
- **Red-Line**: Presentation or IPC polling must NEVER block the Wayland event loop or introduce multi-threaded synchronization primitives into the core reactor.
- **Enforcement**: The reactor loop runs on a single main thread driving `epoll` across Wayland socket, IPC listener, engine child pipes, and one-shot timerfd timers (`[INV-LOOP-01]`, `[INV-LOOP-03]`). Long-running work (e.g. logind D-Bus monitoring) runs in isolated worker threads communicating via non-blocking channels.

### 2.4 CPU Canvas & SHM Buffer Boundary
- **Red-Line**: The Panel presentation path must NEVER create GPU contexts, Vulkan swapchains, or GPU-resident textures ([ADR-0040](../../adr/0040-cpu-canvas-render-shm-buffers.md), [ADR-0053](../../adr/0053-optics-v0.0.62-adoption-boundary.md)).
- **Enforcement**: The candidate popup renders exclusively via `flux_canvas_create_cpu` onto memory-mapped `wl_shm` pool buffers (`[INV-PANEL-13]`). Ink rasterization is 100% CPU-bound, guaranteeing instant response and zero GPU driver context stalls.

---

## 3. Crate Addition and Dependency Rules

1. **Acyclic Dependency Invariant (`[INV-WKS-02]`)**: Lower-layer crates (`typio-host-types`, `typio-engine-protocol`, `typio-engine-manifest`) must NEVER depend on upper layers (`typio-runtime`, `typio-daemon`, `typio-settings`).
2. **Runtime Headless Purity (`[INV-WKS-05]`)**: `typio-runtime` must remain completely decoupled from Wayland, X11, or graphics presentation libraries. It operates entirely as a deterministic state machine.
3. **Optics Rust Bindings Canonical Tagging (`[INV-WKS-06]`)**: Canonical builds resolve Optics bindings from tagged Git releases. Local development links through `.cargo/optics-local.toml` worktree patches without polluting `main`.
4. **Daemon Version Authority (`[INV-WKS-08]`)**: `crates/typio-daemon/Cargo.toml` is the authoritative version source for the Typio product suite.
