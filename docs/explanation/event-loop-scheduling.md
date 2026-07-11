# Event Loop Scheduling

## Purpose

This document defines how the typio event loop schedules work and
preserves responsiveness. It covers the ordering constraints between Wayland
dispatch, Panel rendering, D-Bus, config reload, and voice processing.

The frontend uses one poll loop for Wayland and auxiliary runtime sources.
Auxiliary fds are part of the scheduling model because they can otherwise
delay keymap deadlines, lifecycle cleanup, or user-visible config changes.

## Wayland Dispatch First

Every event-loop iteration dispatches Wayland events before any auxiliary
work. This ordering ensures that input facts are fresh before any non-input
work can delay the focus controller's `reduce`/`diff`/`apply` pipeline.

```text
[Wayland dispatch] ──▶ [record facts] ──▶ [reduce] ──▶ [observe] ──▶ [diff] ──▶ [apply]
                                                            ▲
                                                            │
                                                     [live resources]
```

After the session pipeline completes, auxiliary work runs in a bounded
fashion so no single source can starve the others.

## Reactor Steps and Drivers

One **reactor step** is one execution of the main `while` loop. It is an
implementation scheduling boundary, not a Wayland protocol concept and not a
fixed-rate frame. `poll(2)` may block indefinitely while idle; a readable fd or
an explicit deadline starts the next step.

The loop keeps I/O preparation and phase ordering in
`crates/typio-host/src/app/event_loop.rs`. Cohesive drivers own policy inside
those phases:

| Driver | Responsibility |
|--------|----------------|
| `app/reactor.rs` | Named fd sources, readiness snapshots, earliest-deadline timeout reduction |
| `app/input_driver.rs` | Ordered key batches, engine output, virtual-keyboard forwarding, repeat |
| `keyboard/preedit_coalescer.rs` | Bounded latest-wins pure-preedit staging |
| `app/panel_driver.rs` | Panel scheduler convergence, ownership, anchor, presentation retry |

## Key Drain and Text Transactions

Wayland keyboard-grab events are appended to `InputMethodState::pending_keys`
during dispatch.  The event loop drains that queue in arrival order after the
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
without holding text for compositor `done`. Commit-producing keys bypass the
deadline and flush immediately to preserve order. See
[ADR-0042](../adr/0042-text-input-transaction-staging.md) and
[ADR-0043](../adr/0043-bounded-preedit-coalescing.md).

## Panel Render Bounds

### Panel render cycle

The candidate Panel is rendered at most once per reactor step from the Panel
Scheduler's `IDLE` / `DIRTY` state, never inline in the composition callback or
key routing path.

### CPU frame

The candidate Panel renders on the CPU (flux software canvas + `TextRaster`)
into a premultiplied RGBA8 framebuffer; there is no Vulkan WSI swapchain or
GPU frame to acquire (ADR-0040), so frame setup cannot block the loop.

### SHM attach

The Panel reserves one slot from a triple-buffered `wl_shm` pool before CPU
rendering, byte-swaps the completed framebuffer into it, and attaches it to the
popup surface. If every buffer is busy, the Panel skips rendering and waits for
the next dirty reactor step instead of blocking on compositor release
(ADR-0040, ADR-0044).

### Text rasterisation

Glyphs are shaped and rasterised by flux-text (`TextRaster`) directly into the
panel framebuffer each frame; measurement results are cached, so
repeated candidate pages reuse the measured layout. There is no shared GPU
glyph texture and no per-text-run upload in the CPU-canvas path.

## Poll and Deadline Management

The poll timeout defaults to **`-1` (block until an fd is ready)** so an idle
daemon causes zero wakeups. Sources backed by a `timerfd` — key repeat, the
indicator timer, the config-reload debounce — wake the loop themselves and need
no timeout. Only deadlines *not* backed by an fd shorten the timeout through
the `PollTimeout` earliest-deadline reducer:

- the bounded pure-preedit coalescing deadline,
- the positioned-UI anchor-probe deadline while a popup awaits its caret anchor
  (ADR-0017) — previously covered only implicitly by a fixed baseline tick,
- the virtual-keyboard keymap deadline while the grab is `needs_keymap`.

The loop blocks indefinitely on `poll()` when idle; the `-1` timeout above is
safe because every work stage is non-blocking or bounded (engine IPC at 100 ms,
Wayland I/O non-blocking, config-read at 2 s). See
[Performance & Idle-Power Strategy](performance-strategy.md).

## Auxiliary Source Bounding

### D-Bus dispatch

Tray D-Bus callbacks do not mutate `App` directly. They enqueue typed daemon
events, and the loop drains the channel once per reactor step, keeping D-Bus
threads outside Wayland's thread-affine state.

### Config reload

Config watch events schedule a debounced reload instead of reloading per
inotify event; watches are rearmed after the watched file is deleted, moved,
or replaced by an editor save. Config reload bursts coalesce into a single
runtime reload once the filesystem settles.

### Voice processing

Voice reload is deferred while recording/inference owns the engine snapshot,
then applied once the job completes; the voice fd is refreshed when runtime
config changes.

## Observability

### Log Level Policy

| Level | Purpose |
|-------|---------|
| `debug` | per-event sequencing, repeated grab/keymap churn, routing internals, trace-topic output |
| `info` | low-frequency, user-relevant boundaries: focus changes, grab create/destroy summaries, vk epoch transitions, recovery to `ready` |
| `warning` | recoverable anomalies: repeated grab rebuilds, repeated keymap cancellation before readiness, growing drop counts, fallback paths |
| `error` | fail-safe entry, timeout shutdown, broken invariants, display/protocol failures that stop forwarding |

Operational rules:

- a high-frequency path should not emit one `info` per event
- repeated anomalies prefer one aggregated `warning` plus `debug` detail
- `info` answers "what durable boundary did the frontend just cross?";
  `debug` answers "why, and in what sequence?"

Responsibility split:

- focus-controller effect summaries belong to `crates/typio-host/src/app/event_loop.rs`
- teardown-cause and grab create/destroy logs belong to `crates/typio-host/src/session_glue.rs` / `focus_controller.rs`
- virtual-keyboard health and fail-safe logs belong to the platform input-method bridge in `crates/typio-host-platform/src/input_method.rs`
- per-key sequencing and modifier-path traces belong to `crates/typio-host/src/keyboard/router.rs`

Do not duplicate one transition across layers at the same log level. Prefer
`debug` detail in a helper and one `info` summary at the boundary owner.

### Trace Capture

For shortcut-routing or repeat bugs:

```sh
typio --verbose 2>&1 | tee typio-trace.log
```

Read traces in this order: sort by `seq`, group by `topic`, compare
`grab_state`, `active_key_generation`, `mods`, `phys`, and `xkb`. For
`Ctrl+T`-style bugs, inspect `TRACE key`, `TRACE vk_key`, and
`TRACE vk_modifiers`. A release whose stored key generation does not match
`active_key_generation` is a cross-boundary orphan and is expected to be
dropped at routing.

### Runtime Diagnostics

`RuntimeState` exports the projection of frontend fields and observed
lifecycle axes. The highest-value fields:

| Field | Meaning |
|-------|---------|
| `lifecycle_phase` | `inactive` / `activating` / `active` / `deactivating` |
| `grab_state` | `absent` / `needs_keymap` / `ready` / `broken` |
| `active_key_generation` | current grab epoch |
| `keyboard_grab_active` | whether a grab object exists |
| `virtual_keyboard_state` | vk readiness |
| `virtual_keyboard_has_keymap` | vk has received a keymap |
| `virtual_keyboard_keymap_generation` | keymap epoch |
| `virtual_keyboard_drop_count` | cumulative dropped forwards |
| `virtual_keyboard_state_age_ms` | time since last vk state change |
| `virtual_keyboard_keymap_deadline_remaining_ms` | time until keymap timeout |

A healthy active session: `lifecycle_phase=active`, `grab_state=ready`,
`keyboard_grab_active=true`, `virtual_keyboard_state=ready`, `drop_count`
stable. `grab_state=needs_keymap` while focused for longer than the keymap
deadline is the primary clue that the grab→keymap→vk chain did not close.

## Invariants

- config reload bursts coalesce into a single runtime reload once the
  filesystem settles
- the Panel's CPU render + SHM attach path runs on the loop thread and must
  stay bounded
- text is rasterised by `TextRaster` into the framebuffer; glyph *measurement*
  is cached, rasterisation is per-frame

## See Also

- [Input-Method Session](input-method-session.md) — the three layers of
  "session", build-up chain, and lifecycle rules
- [Focus Controller](focus-controller.md) — the reduce/diff/apply pipeline
  that runs inside the event loop
- [Wayland Input Method Protocol](wayland-input-method.md) — protocol
  implementation and event handlers
- [Panel Architecture](panel-architecture.md) — Panel content, zones, and
  rendering
- [ADR-0004: Event Loop Scheduling and Watchdog](../adr/0004-event-loop-scheduling-and-watchdog.md) (watchdog part superseded by ADR-0041)
- [ADR-0024: Idle-Driven Event Loop and Demand-Gated Watchdog](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md) (watchdog part superseded by ADR-0041)
- [Performance & Idle-Power Strategy](performance-strategy.md)
