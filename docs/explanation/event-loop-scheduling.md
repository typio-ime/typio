# Event Loop Scheduling

## Purpose

This document defines how the typio event loop schedules work and
preserves responsiveness. It covers the ordering constraints between Wayland
dispatch, Panel rendering, D-Bus, config reload, and voice processing.

The frontend uses one poll loop for Wayland and auxiliary runtime sources.
Auxiliary descriptors are part of the scheduling model because they can otherwise
delay keymap deadlines, lifecycle cleanup, or user-visible config changes.

## Wayland Dispatch First

Every event-loop iteration dispatches Wayland events before any auxiliary
work. This ordering ensures that input facts are fresh before any non-input
work can delay the focus controller's derive/diff/apply pipeline. The pipeline
itself runs in a fixed order:

1. Dispatch queued Wayland events.
2. Record the input facts those events carry.
3. Derive the desired resource configuration from the facts.
4. Observe the live resources.
5. Diff desired against actual to obtain a minimal effect set.
6. Apply the effects to the live resources.

After the session pipeline completes, auxiliary work runs in a bounded
fashion so no single source can starve the others.

## Reactor Steps and Drivers

One **reactor step** is one execution of the main loop. It is an
implementation scheduling boundary, not a Wayland protocol concept and not a
fixed-rate frame. The poll call may block indefinitely while idle; a readable
descriptor or an explicit deadline starts the next step.

The loop keeps I/O preparation and phase ordering in one place. Cohesive
drivers own policy inside those phases:

- the **reactor** owns the named descriptor sources, the readiness snapshot, and
  the earliest-deadline timeout reduction;
- the **input driver** owns ordered key batches, engine output, virtual-keyboard
  forwarding, and key repeat;
- the **preedit coalescer** owns bounded latest-wins staging of pure preedit;
- the **panel driver** owns panel schedule convergence, ownership, anchor
  handling, and presentation retry.

## Key Drain and Text Transactions

Wayland keyboard-grab events are appended to a pending-key queue during
dispatch. The event loop drains that queue in arrival order after the
focus-controller pipeline has converged.

Engine output is drained after each consumed key, but compositor-facing text
commits are not blindly flushed after every composition:

- commit text is staged and flushed before the next key is routed, preserving
  strict text order;
- composition-only preedit updates are staged in a latest-wins coalescer that
  spans adjacent pending-key drains;
- candidate state and panel dirtiness are updated immediately in host memory, so
  panel rendering still sees the latest candidates in the same loop iteration.

Pure preedit becomes eligible after 2 ms without another update and has a fixed
4 ms maximum delay from the first update in a burst. A later update replaces
the payload and renews only the quiet deadline. This catches physically
adjacent keys even when Wayland delivers them in different reactor steps,
without holding text for the compositor's commit point. Commit-producing keys
bypass the deadline and flush immediately to preserve order. See
[ADR-0042](../adr/0042-text-input-transaction-staging.md) and
[ADR-0043](../adr/0043-bounded-preedit-coalescing.md).

## Panel Render Bounds

### Panel Render Cycle

The candidate Panel is rendered at most once per reactor step from the panel
schedule state (idle or dirty), never inline in the composition callback or
key routing path.

### CPU Frame

The candidate Panel renders on the CPU (flux software canvas plus CPU text
rasterisation) into a premultiplied RGBA8 framebuffer; there is no GPU
frame to acquire ([ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md)), so
frame setup cannot block the loop.

### Shared-Memory Attach

The Panel reserves one slot from a triple-buffered shared-memory pool before CPU
rendering, byte-swaps the completed framebuffer into it, and attaches it to the
popup surface. If every buffer is busy, the Panel skips rendering and waits for
the next dirty reactor step instead of blocking on compositor release
([ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md), [ADR-0044](../adr/0044-bounded-panel-rendering.md)).

### Text Rasterisation

Glyphs are shaped and rasterised by flux-text directly into the panel
framebuffer each frame; measurement results are cached, so
repeated candidate pages reuse the measured layout. There is no shared GPU
glyph texture and no per-text-run upload in the CPU-canvas path.

## Poll and Deadline Management

The poll timeout defaults to **unbounded — block until a descriptor is ready**
so an idle daemon causes zero wakeups. Sources that own their own timer
descriptor — key repeat, the indicator timer, the config-reload debounce — wake
the loop themselves and need no timeout. Only deadlines *not* backed by a
descriptor shorten the timeout through the earliest-deadline reducer:

- the bounded pure-preedit coalescing deadline,
- the positioned-UI anchor-probe deadline while a popup awaits its caret anchor
  ([ADR-0017](../adr/0017-positioned-ui-arbitration.md)) — previously covered
  only implicitly by a fixed baseline tick,
- the virtual-keyboard keymap deadline while the grab still awaits its keymap.

The loop blocks indefinitely when idle; that is safe because every work
stage is non-blocking or bounded at its call site — engine IPC at 50 ms
for keystrokes and 100 ms for availability, Wayland I/O non-blocking,
config read at 2 s. See
[Performance & Idle-Power Strategy](performance-strategy.md).

## Auxiliary Source Bounding

### D-Bus Dispatch

Tray D-Bus callbacks do not mutate the application state directly. They enqueue
typed daemon events, and the loop drains the channel once per reactor step,
keeping D-Bus threads outside Wayland's thread-affine state.

### Config Reload

Config watch events schedule a debounced reload instead of reloading per
inotify event; watches are rearmed after the watched file is deleted, moved,
or replaced by an editor save. Config reload bursts coalesce into a single
runtime reload once the filesystem settles.

### Voice Processing

Voice reload is deferred while recording or inference owns the engine snapshot,
then applied once the job completes; the voice descriptor is refreshed when
runtime config changes.

## Observability

### Log Level Policy

| Level | Purpose |
|-------|---------|
| `debug` | per-event sequencing, repeated grab/keymap churn, routing internals, trace-topic output |
| `info` | low-frequency, user-relevant boundaries: focus changes, grab create/destroy summaries, virtual-keyboard epoch transitions, recovery to a ready grab |
| `warning` | recoverable anomalies: repeated grab rebuilds, repeated keymap cancellation before readiness, growing drop counts, fallback paths |
| `error` | fail-safe entry, timeout shutdown, broken invariants, display/protocol failures that stop forwarding |

Operational rules:

- a high-frequency path should not emit one `info` per event
- repeated anomalies prefer one aggregated `warning` plus `debug` detail
- `info` answers "what durable boundary did the frontend just cross?";
  `debug` answers "why, and in what sequence?"

Responsibility split:

- focus-controller effect summaries belong to the event-loop owner
- teardown-cause and grab create/destroy logs belong to the session-glue and
  focus-controller layer
- virtual-keyboard health and fail-safe logs belong to the platform
  input-method bridge
- per-key sequencing and modifier-path traces belong to the keyboard router

Do not duplicate one transition across layers at the same log level. Prefer
`debug` detail in a helper and one `info` summary at the boundary owner.

### Trace Capture

For shortcut-routing or repeat bugs, run the daemon with verbose logging and
capture standard error to a file; the
[Troubleshooting Guide](../how-to/troubleshooting.md) gives the exact pipeline
and the log-level mapping. Read traces in this order: sort by sequence number,
group by topic, then compare the grab state, the active key generation, the
modifier mask, and the physical and logical key fields. For chord-level bugs,
inspect the key, virtual-key, and virtual-modifier trace records. A release
whose stored key generation does not match the active generation is a
cross-boundary orphan and is expected to be dropped at routing.

### Runtime State Projection

The control surface exports a read-only projection of the frontend's observed
lifecycle axes, never a second tracker. The highest-value axes are:

- the lifecycle phase, as one of inactive, activating, active, or deactivating;
- the grab state, as one of absent, awaiting keymap, ready, or broken;
- the active key generation — the current grab epoch — and whether a grab
  object exists at all;
- virtual-keyboard readiness, whether a keymap has been received, and its
  keymap generation;
- the cumulative count of dropped virtual-keyboard forwards;
- the age of the last virtual-keyboard state change and the time remaining on
  the keymap deadline.

A healthy active session reports the active phase, a ready grab, a present grab
object, a ready virtual keyboard, and a stable drop count. A grab that stays in
the awaiting-keymap state for longer than the keymap deadline while the field is
focused is the primary clue that the grab-to-keymap-to-virtual-keyboard chain
did not close.

## Invariants

- config reload bursts coalesce into a single runtime reload once the
  filesystem settles
- the Panel's CPU render plus shared-memory attach path runs on the loop thread
  and must stay bounded
- text is rasterised into the framebuffer; glyph *measurement* is cached,
  rasterisation is per-frame

## See Also

- [Input-Method Session](input-method-session.md) — the three layers of
  "session", build-up chain, and lifecycle rules
- [Focus Controller](focus-controller.md) — the derive/diff/apply pipeline
  that runs inside the event loop
- [Wayland Input Method Protocol](wayland-input-method.md) — protocol
  implementation and event handlers
- [Panel Architecture](panel-architecture.md) — Panel content, zones, and
  rendering
- [Daemon Lifecycle Blueprint](../architecture/daemon-lifecycle.md) — the
  reactor step, deadline table, and latency budgets in force today
- [ADR-0004: Event Loop Scheduling and Watchdog](../adr/0004-event-loop-scheduling-and-watchdog.md) (watchdog part superseded by ADR-0041)
- [ADR-0024: Idle-Driven Event Loop and Demand-Gated Watchdog](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md) (watchdog part superseded by ADR-0041)
- [Performance & Idle-Power Strategy](performance-strategy.md)
