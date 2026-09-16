# ADR-0048: Modern Optics Lens Component Alignment and Bounded Keystroke Latency Budget

- **Status**: Accepted
- **Date**: 2026-09-11
- **Deciders**: Core Maintainers

## Context

1. **Optics API Drift**:
   Typio's graphics stack depends on the sibling `optics` monorepo (`flux`, `flux-text`, `iris`,
   `lens`). The workspace `Cargo.toml` and CI pipeline were historically pinned to `v0.0.29`.
   Starting with Optics `v0.0.30` (ADR-0081 and ADR-0082 in Optics), compound microkernel
   widgets such as `f.collapsing(...)` were pruned from `liblens` core in favor of orthogonal
   userland compositions, and `f.title(...)` was superseded by `f.heading(...)`. Enabling local
   cross-repository worktrees (`.cargo/optics-local.toml`) caused `crates/typio-settings` to fail
   compilation with 9 errors.

2. **Keystroke Latency Budget**:
   `crates/typio-runtime`'s `ProcessBackend` previously used a broad 100 ms timeout
   (`ENGINE_REQUEST_TIMEOUT`) across both hot-path keystroke processing (`process-key`) and
   auxiliary queries (`availability`). Under an engine stall or heavy disk contention, a 100 ms
   block directly freezes the Wayland event loop, delaying input echo across multiple display frames.

## Decision

1. **Align `typio-settings` with Modern Optics (v0.0.37)**:
   - Replace deprecated `f.title(...)` calls with `f.heading(..., level)`.
   - Implement an explicit state-managed `section_disclosure` pattern in `AppState` using
     `button_subtle` and collapsible content panels, fully conforming to Optics ADR-0082.
   - Upgrade workspace dependencies and CI pinned commit to Optics `v0.0.37` (`ff6246f`).
2. **Bound Keystroke Latency to 50 ms (`ENGINE_KEY_TIMEOUT`)**:
   - Establish a dedicated `ENGINE_KEY_TIMEOUT = Duration::from_millis(50)` for `"process-key"`.
   - Keystroke queries exceeding 50 ms immediately trigger poison recovery, terminating the
     stalled child worker and respawning asynchronously on a detached thread while allowing
     unhandled keys to pass straight through to the focused application.

## Alternatives considered

- **Retain legacy Optics v0.0.29 indefinitely**: Would permanently orphan Typio's graphical
  components from modern upstream compositor fixes, font rendering optimizations, and memory
  safety patches.
- **Asynchronous keystroke queuing with predictive typing**: Highly complex across Wayland
  `zwp_virtual_keyboard_v1` and unhandled key fallbacks; a tight 50 ms synchronous deadline
  with asynchronous background respawn provides immediate resilience without risking out-of-order
  synthesized key events.

## Consequences

- Positive: `typio-settings` builds cleanly both in standalone pinned builds and with linked
  local Optics worktrees.
- Positive: Maximum worst-case reactor pause from an unresponsive engine is halved from 100 ms
  to 50 ms.
- Positive: Workspace is fully compatible with Optics `v0.0.37`.
