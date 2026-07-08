# ADR-0041: Remove the host watchdog

- **Status**: Accepted
- **Date**: 2026-07-08
- **Deciders**: Typio maintainers
- **Supersedes**: [ADR-0004](0004-event-loop-scheduling-and-watchdog.md) (watchdog
  part), [ADR-0024](0024-idle-driven-loop-and-demand-gated-watchdog.md),
  [ADR-0037](0037-demand-armed-watchdog-cadence.md)

## Context

ADR-0004 introduced a per-stage heartbeat watchdog: a background thread sampled
the main loop's progress and `SIGKILL`ed the daemon if a non-restful stage held
without a heartbeat past a stuck threshold (3 s), on the theory that a
grab-holding input method that wedges locks the user out of typing. ADR-0024
made it demand-gated (condvar-blocked while disarmed, zero idle wakeups);
ADR-0037 refined the arm/disarm edges to focus transitions and the cadence to
0.5 Hz.

Over its lifetime the watchdog grew to **327 lines** (`watchdog.rs`: a thread,
a condvar, an 11-variant stage enum, five unit tests) plus **39 call sites**
spread across seven files — every stage of the main loop carried an intrusive
`wd!().set_stage(...)` annotation. This cross-cutting complexity was the
motivation for re-examining it.

A full audit of the main loop's blocking points found that **the risk the
watchdog was built to guard had already been closed by other mechanisms**:

| Stage | Operation | Blocking? | Evidence |
|-------|-----------|-----------|----------|
| Repeat / key path | engine IPC `read_exact` | **bounded at 100 ms** | `ENGINE_REQUEST_TIMEOUT` (`process.rs:30`) — `set_read_timeout` on the socketpair |
| Flush | `Connection::flush` → `wl_display_flush` | non-blocking | returns `WouldBlock`/EAGAIN on a full socket; data stays in libwayland's buffer for retry |
| ReadEvents | `wl_display_read_events` | non-blocking | reads only already-ready data (the loop `poll`s POLLIN first) |
| DispatchPending | `wl_display_dispatch_queue_pending` | non-blocking | drains only already-buffered events; no dispatched callback issues a compositor round-trip |
| AuxIo | focus-controller tick | non-blocking | in-memory state machine |
| PanelUpdate / Present | CPU canvas + `wl_shm` attach | non-blocking | ADR-0040 removed the GPU/Vulkan present path and its readback stall |
| ConfigReload | `typio_instance_reload_config` | **was blocking** (one `read_to_string`) | **fixed in this change** — see Decision |

The watchdog's strongest historical guard — the `Present`-stage GPU-fence
deadlock — was already eliminated by ADR-0040. Its headline remaining guard —
engine-IPC stalls — was already bounded by the 100 ms `ENGINE_REQUEST_TIMEOUT`.
Of the 11 stages, **only `ConfigReload` had a genuine unbounded blocking point**
(one raw `fs::read_to_string`), and only in the NFS/FUSE-stall pathological
case.

The audit also found a watchdog **blind spot**: the synchronous `zbus` signal
emissions on the `StateRefresh` hot path ran with the stage reading `Idle`
(restful, exempt), so the watchdog did not cover them either. The watchdog was
therefore neither cheap nor a complete deadlock defence.

## Decision

Remove the watchdog entirely, and close the one real blocking point it guarded
at the source.

1. **Delete `watchdog.rs`** and every `wd!()` / `set_stage` / `set_armed`
   annotation from the event loop. The main-loop skeleton is now free of
   cross-cutting instrumentation.

2. **Bound `ConfigReload`** — the one genuine blocking point. `typio_config_load_file`
   now reads the file on a short-lived reader thread with a 2 s channel timeout
   (`config.rs`). A stalled NFS/FUSE mount fails the load instead of hanging
   the loop; the previous config is retained.

3. **Remove the per-draw `heartbeat`/`before_present` callbacks** from
   `FluxPanel::draw_candidates` and `draw_status_banner`. They existed solely
   to advance the watchdog heartbeat during panel rendering; with the panel
   path non-blocking (ADR-0040) they were dead weight in every call.

4. **Drop the `watchdogArmed` IPC field** — it was never wired to real armed
   state (no code path ever set it true) and now has nothing to report.

5. **Stall attribution is retained via the existing `tracing` targets**
   (`typio.wayland.io`, `typio.engine.key`, `typio.config`, …). When a future
   hang occurs, the last logged target names the stage; no per-stage
   instrumentation is reintroduced.

## What is deliberately not done

- **No replacement self-heal (`SIGKILL` + restart) path.** The systemd user
  service (ADR-0021) still restarts the daemon on a crash, but a *wedge* (the
  process alive but stuck) is no longer auto-recovered. This is accepted: the
  audit established that no main-loop stage can wedge on healthy code paths,
  and the single pathological case (NFS-stall config read) is now bounded.
  Future blocking calls must be caught in review, not at runtime.

- **No async engine IPC.** The key path stays synchronous (`process_key` →
  request/reply with a 100 ms timeout). As analyzed in the review, full
  async (buffer every key, decide on reply) would add a poll-tick of latency
  to 100 % of keystrokes to defend against a stall already bounded at 100 ms —
  a losing trade for a latency-critical input path.

## Consequences

- Positive: **−327 lines** and **−39 intrusive annotations** across seven
  files. The main-loop reads as a plain flush→read→dispatch→poll→drain
  pipeline with no cross-cutting machinery.
- Positive: no background thread, no condvar, no focus-transition arming
  coupling. One fewer subsystem to reason about in the most latency-critical
  code.
- Positive: the NFS-stall config-read risk is closed at the source rather than
  compensated for at runtime.
- Trade-off: a future change that reintroduces a blocking call on the main
  loop has no runtime safety net. This is mitigated by the audit baseline
  (every stage now non-blocking or bounded) and the discipline of keeping it
  so; the `ENGINE_REQUEST_TIMEOUT` precedent shows the intended pattern
  (bound blocking at the call site).
- Negative (accepted): a wedged daemon no longer self-terminates. Recovery is
  manual (the compositor's VT-switch hotkeys bypass the IME grab; `pkill`
  from a TTY; or systemd `Restart=on-failure` only helps on crash, not wedge).

## Related

- [ADR-0004](0004-event-loop-scheduling-and-watchdog.md) — introduced the
  watchdog and event loop; superseded here.
- [ADR-0024](0024-idle-driven-loop-and-demand-gated-watchdog.md) — made it
  demand-gated; superseded here.
- [ADR-0037](0037-demand-armed-watchdog-cadence.md) — refined the cadence;
  superseded here.
- [ADR-0040](0040-cpu-canvas-render-shm-buffers.md) — removed the GPU present
  path, eliminating the watchdog's strongest historical guard target.
- [ADR-0021](0021-systemd-user-service-daemon-lifecycle.md) — the restart
  path, which now only covers crashes, not wedges.
