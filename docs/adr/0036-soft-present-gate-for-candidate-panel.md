# ADR-0036: Soft Present Gate for the Candidate Panel

- **Status**: Accepted
- **Date**: 2026-06-30
- **Deciders**: Typio maintainers
- **Amends**: [ADR-0010](0010-non-blocking-candidate-popup-present.md), [ADR-0023](0023-panel-scheduler-state-machine.md)

## Context

The candidate panel uses a Wayland input-popup surface backed by the Flux
Vulkan renderer. Flux presents synchronously on the input-method event loop, so
the host must avoid submitting panel frames faster than the compositor releases
swapchain images.

The previous mitigation armed `wl_surface.frame` after every panel present and
treated the callback as a hard lock: while `done` was outstanding, candidate
updates stayed dirty but did not present. A 200 ms watchdog recovered if the
callback stayed pending too long.

Long Rime candidate-switching diagnostics showed that this hard lock was still
visible to users. When the compositor stopped delivering frame callbacks for
the popup, the panel waited for the 200 ms recovery path. The logs showed
`frame-callback stall` warnings with 200-600 ms callback gaps, while the glyph
atlas did not clear and Rime key processing stayed around 10 ms. The callback
stall, not engine compute, explained the large freezes.

`wl_surface.frame` is not a reliable liveness contract for an input-popup
surface. A compositor may deprioritize, occlude, or mishandle the popup and
withhold callbacks even though presenting a newer candidate frame is still the
right user-visible behavior.

## Decision

Treat `wl_surface.frame` as a soft present gate, not a hard lock.

The event loop now asks a pure present-gate policy whether to present or wait:

- if no frame callback is outstanding, present immediately;
- if a frame callback is outstanding and still within the soft limit, keep the
  candidate snapshot dirty and wait until the callback arrives or the deadline
  expires;
- if the soft limit expires, present the latest coalesced candidate state
  anyway and re-arm a fresh callback;
- if callbacks remain absent for the diagnostic threshold, log one
  `frame-callback stall` warning for that missing-callback episode, but do not
  block rendering.

The soft limit is 50 ms, longer than one 30 Hz frame but still far below the
old 200 ms recovery path. Healthy compositors wake earlier through the
callback. A compositor that drops callbacks timer-paces candidate navigation at
a conservative cadence instead of filling the swapchain and blocking in
present.

The panel scheduler remains the owner of dirty/retry state from ADR-0023. The
present gate only contributes a poll deadline while candidates are dirty, so
the event loop wakes when the soft limit expires even if no Wayland event
arrives.

## Alternatives considered

- **Keep the 200 ms hard-lock watchdog**: Rejected because the recovery delay is
  directly user-visible during candidate navigation.
- **Lower the hard-lock timeout**: Rejected because it keeps the wrong
  invariant. The issue is treating callback delivery as required for progress,
  not the exact timeout value.
- **Ignore `wl_surface.frame` entirely and use a fixed timer**: Rejected because
  healthy compositors provide useful refresh pacing. The callback should still
  wake the panel early when it is reliable.
- **Present every dirty candidate update immediately**: Rejected because rapid
  paging can reintroduce synchronous present back-pressure on the event loop.

## Consequences

- Positive: A missing compositor frame callback no longer freezes candidate
  updates until a 200 ms watchdog path.
- Positive: Healthy compositors still pace the panel through
  `wl_surface.frame`, preserving the swapchain back-pressure protection from
  ADR-0010.
- Positive: The missing-callback condition remains visible through structured
  warnings and a running `stall_count`.
- Trade-off: When a compositor drops callbacks, the host may present at the
  soft-limit cadence instead of the compositor's actual refresh cadence.
- Negative (accepted): The input-method frontend keeps additional state for a
  missing-callback episode so diagnostics are not reset by soft-gate re-arms.
