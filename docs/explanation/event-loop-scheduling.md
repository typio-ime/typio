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

## Panel Render Bounds

### Panel render cycle

The candidate Panel is rendered once per loop iteration from the Panel
Scheduler's `DIRTY` / `RETRY` state, never inline in the composition callback
or key routing path.

### CPU frame

The candidate Panel renders on the CPU (flux software canvas + `TextRaster`)
into a premultiplied RGBA8 framebuffer; there is no Vulkan WSI swapchain or
GPU frame to acquire (ADR-0040), so frame setup cannot block the loop.

### SHM attach

The framebuffer is byte-swapped into a double-buffered `wl_shm` pool and
attached to the popup surface. If every SHM buffer is still busy, the panel
drops the frame and waits for the next dirty tick instead of blocking on
compositor buffer release (ADR-0040).

### Frame callback pacing

`wl_surface.frame` callbacks pace healthy compositors at refresh rate. Missing
callbacks are treated as a soft gate: the event loop wakes on a deadline and
submits the latest coalesced candidate state rather than freezing.

### Text rasterisation

Glyphs are shaped and rasterised by flux-text (`TextRaster`) directly into the
panel framebuffer each frame; measurement results are cached, so
repeated candidate pages reuse the measured layout. There is no shared GPU
glyph texture and no per-text-run upload in the CPU-canvas path.

## Poll and Deadline Management

The poll timeout defaults to **`-1` (block until an fd is ready)** so an idle
daemon causes zero wakeups. Sources backed by a `timerfd` — key repeat, the
indicator timer, the config-reload debounce — wake the loop themselves and need
no timeout. Only deadlines *not* backed by an fd shorten the timeout, each via a
`-1`-aware minimum (`poll_timeout_min`):

- the panel frame-callback soft gate while a deferred flush is pending,
- the positioned-UI anchor-probe deadline while a popup awaits its caret anchor
  (ADR-0017) — previously covered only implicitly by a fixed baseline tick,
- the virtual-keyboard keymap deadline while the grab is `needs_keymap`.

The loop blocks indefinitely on `poll()` when idle; the `-1` timeout above is
safe because every work stage is non-blocking or bounded (engine IPC at 100 ms,
Wayland I/O non-blocking, config-read at 2 s). See
[Performance & Idle-Power Strategy](performance-strategy.md).

## Auxiliary Source Bounding

### D-Bus dispatch

Status and tray D-Bus dispatch are bounded per tick so a busy bus cannot
starve Wayland dispatch, voice completion, repeat, or config reload.

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

- focus-controller effect summaries belong to `event_loop.c`
- teardown-cause and grab create/destroy logs belong to `focus_effects.c`
- virtual-keyboard health and fail-safe logs belong to `bridge.c`
- per-key sequencing and modifier-path traces belong to `keyboard.c`
- watchdog and dispatch-path logs belong to `event_loop.c`

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
