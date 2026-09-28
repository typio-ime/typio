---
id: ADR-0022
title: "Panel retry result owned by update"
status: accepted
date: 2026-06-03
scope: panel
---

# ADR-0022: Panel retry result owned by update

- **Status**: Accepted
- **Date**: 2026-06-03
- **Deciders**: typio-linux maintainers
- **Amends**: [ADR-0015](0015-candidate-popup-lag-final-fixes.md)

## Context

ADR-0015 introduced `present_retry` as a persistent flag on
`TypioPanelSurface`. The event loop skipped Panel flushes while that flag was
set, and `panel_render()` cleared it at the beginning of the next render.

That created a latch:

1. `panel_surface_present()` returned `PANEL_PRESENT_RETRY`.
2. `TypioPanelSurface.present_retry` became true.
3. The event loop skipped the next Panel flush because retry was pending.
4. `panel_render()` did not run, so the retry flag could not be cleared.

The visible symptom matched the long-session candidate report: logical
selection and commit could continue, but the visible candidate highlight stopped
catching up after a compositor/swapchain stall.

## Decision

Make retry a result of one Panel update, not durable state on the surface.

`panel_surface_present()` still returns `PANEL_PRESENT_RETRY`, but the surface no
longer stores a retry-pending flag. `typio_panel_update()` and
`typio_panel_update_content()` return `TypioPanelUpdateResult`
(`OK`, `RETRY`, `FAIL`). The frontend keeps `panel_update_pending` set only when
the current update returned `RETRY`.

When a retry remains pending, the event loop shortens its poll timeout to 16 ms.
That gives presentation a prompt future attempt without busy-spinning or
blocking every key event on repeated acquire timeouts.

## Alternatives considered

- **Keep the persistent retry flag and clear it from the event loop**: Rejected
  because it keeps ownership split across surface, Panel, and frontend layers.
  The surface should report the result of one present attempt, not coordinate
  scheduling.
- **Keep skipping flushes until a new input event arrives**: Rejected because a
  stalled Panel might remain stale when no further key event arrives after the
  compositor recovers.
- **Retry immediately in a tight loop**: Rejected because a compositor that is
  still not releasing images would consume the event loop with bounded but
  repeated acquire attempts.

## Consequences

- Positive: A single `PANEL_PRESENT_RETRY` can no longer permanently suppress
  future Panel renders.
- Positive: Retry scheduling is owned by the frontend update loop, while the
  surface owns only present mechanics.
- Positive: The obsolete `present_retry` field and pending/reset accessors are
  removed.
- Trade-off: During a real compositor stall, the event loop wakes at a 16 ms
  retry cadence while `panel_update_pending` remains set.
- Negative (accepted): `typio_panel_update()` no longer has a boolean result;
  callers that care about retry must handle the explicit three-state result.
