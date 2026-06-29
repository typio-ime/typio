# ADR-0037: Demand-Armed Watchdog Cadence

- **Status**: Accepted
- **Date**: 2026-06-30
- **Deciders**: Typio maintainers
- **Amends**: [ADR-0024](0024-idle-driven-loop-and-demand-gated-watchdog.md)

## Context

ADR-0024 made the watchdog block while disarmed and sample at 1 s while armed.
The Rust frontend still armed the watchdog immediately after Wayland startup,
which meant the thread sampled even when no input field was focused. That was
lighter than the historical 100 ms tick, but it did not fully implement the
demand-gated model.

The candidate-panel soft present gate in
[ADR-0036](0036-soft-present-gate-for-candidate-panel.md) also removes the
watchdog from ordinary frame-callback recovery. The watchdog remains valuable
for true event-loop wedges, but it no longer needs to be tuned as a
low-latency panel recovery mechanism.

## Decision

Start the watchdog disarmed.

The event loop arms the watchdog on `FirstActivate` and `Reactivate` focus
transitions, and disarms it on `Deactivate`. While armed, the production sample
interval is 2 s. The stuck threshold remains 3 s for ordinary work stages and
15 s for `Present`.

This keeps idle cost at zero wakeups, lowers active focused sampling overhead,
and preserves recovery for genuine grab-holding stalls.

## Alternatives considered

- **Remove the watchdog**: Rejected because a grab-holding input method needs a
  self-heal path for driver, compositor, or IPC stalls.
- **Keep startup-armed behavior**: Rejected because it samples when there is no
  focused input work to protect.
- **Keep the 1 s armed cadence**: Rejected because the soft present gate now
  handles ordinary candidate-panel callback loss; the watchdog can use a
  coarser safety cadence.
- **Raise the stuck threshold instead of the sample interval**: Rejected
  because the threshold encodes how long the daemon may hold the keyboard grab
  while wedged. Sampling less often lowers overhead without changing that
  semantic threshold.

## Consequences

- Positive: No-input-focus operation has zero watchdog wakeups.
- Positive: Focused operation samples at 0.5 Hz instead of 1 Hz.
- Positive: Genuine non-restful work-stage stalls still terminate and rely on
  the systemd user service restart path.
- Trade-off: Detection latency widens slightly. A 3 s stuck threshold is
  observed on the next 2 s sample, so practical detection is roughly 3-5 s.
- Negative (accepted): Focus transitions now own watchdog arming, so regressions
  in focus-transition handling can leave the watchdog disarmed until the next
  activation edge.
