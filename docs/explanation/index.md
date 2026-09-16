# Explanation

Charter: this directory holds **understanding-oriented** documents. Every page
here answers *why the system is shaped this way* and how its parts relate. It
is deliberately free of implementation coordinates — no source paths, symbol
names, or code blocks — so that it stays readable after a refactor. The
[Architecture Blueprints](../architecture/index.md) carry the current structure,
and source-level coordinates live in the contributor plane.

If you want to *do* something, start from [How-to Guides](../how-to/index.md).
If you want a value or a signature, use [Reference](../reference/index.md).

## System shape

| Page | Explains |
| :--- | :--- |
| [Project Scope](project-scope.md) | The repository's boundaries: daemon, runtime, engine protocol, and clients |
| [Runtime Architecture](architecture-overview.md) | How the Wayland host, the in-process runtime, the engine protocol, and the control surface divide responsibility |
| [Security Model](security-model.md) | Trust boundaries, what a malicious engine or compositor can reach, and what is deliberately out of scope |

## Input and session lifecycle

| Page | Explains |
| :--- | :--- |
| [Wayland Input Method Protocol](wayland-input-method.md) | Why `zwp_input_method_v2` is the host protocol and how its serials gate every text commit |
| [Wayland Input-Method v2 Session](input-method-session.md) | The three meanings of "session", the build-up chain, and lifecycle rules |
| [Focus Controller](focus-controller.md) | Derived state and idempotent diffs: how lifecycle state is computed instead of stored |
| [Modifier-Key Consumption](modifier-key-consumption.md) | How modifier keys follow the engine's result contract instead of a host-side guess |
| [Composition State Machine](composition-state-machine.md) | Composition as state and commit as an ordered event |
| [Voice Input](voice-input.md) | The capture, inference, and delivery split for dictation |
| [Engine Contract](engine-contract.md) | The process boundary between host and engine, and what crosses it |

## Presentation

| Page | Explains |
| :--- | :--- |
| [Panel Architecture](panel-architecture.md) | The multi-zone Panel and the arbitration that decides which producer is visible |
| [Candidate Panel Behavior](candidate-panel-behavior.md) | The visible lifecycle: show, hide, anchor, retry, and the effect of input on screen |
| [Frontend Graphics](frontend-graphics.md) | Why the Panel rasterises on the CPU and presents over shared memory |
| [Performance & Idle-Power Strategy](performance-strategy.md) | Event-driven idle, deadline folding, and how to measure wakeups |

## Operations and control

| Page | Explains |
| :--- | :--- |
| [Event Loop Scheduling](event-loop-scheduling.md) | One poll loop, bounded work per tick, and how deadlines are folded into the timeout |
| [Stall Containment](stall-containment.md) | Why the host has no runtime self-heal, and how stalls are attributed instead |
| [Control Surfaces](control-surfaces.md) | Which interfaces exist, who may call them, and how they stay consistent |
| [Configuration System](configuration-system.md) | One persisted configuration, two schema sources, and atomic per-engine slots |
| [Engine-to-Host Resource Flow](engine-host-resource-flow.md) | For every shared resource, who is the authority and who validates |

## See also

- [Architecture Blueprints](../architecture/index.md) — the current state of each subsystem
- [ADR Index](../adr/index.md) — the immutable decisions behind these designs
- [Reference](../reference/index.md) — exact keys, verbs, and wire formats
