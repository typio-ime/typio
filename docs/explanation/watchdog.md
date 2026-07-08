# Watchdog (removed)

The host watchdog — a background thread that sampled the main loop's progress
and `SIGKILL`ed the daemon on a stalled work stage — has been **removed**
([ADR-0041](../adr/0041-remove-watchdog.md)).

## Why it existed

ADR-0004 introduced it on the theory that a grab-holding input method that
wedges locks the user out of all typing, and that an independent thread
detecting the stall + `SIGKILL` (with the systemd user service restarting the
daemon) was the one self-heal for that worst case.

## Why it was removed

A full audit of the main loop's blocking points established that the risk it
guarded had already been closed by other mechanisms:

- **Engine IPC stalls** — the strongest historical motivator — are bounded at
  100 ms by `ENGINE_REQUEST_TIMEOUT` on the worker socketpair.
- **GPU/Vulkan present deadlocks** were eliminated by ADR-0040 (CPU canvas +
  non-blocking `wl_shm` attach).
- **Wayland I/O** (`flush`, `read_events`, `dispatch_pending`) is non-blocking
  by construction.
- **Config reload** — the one genuine unbounded blocking point (a raw
  `read_to_string` on a possibly NFS/FUSE-mounted file) — is now bounded by a
  2 s reader-thread timeout at the source.

With every main-loop stage non-blocking or bounded, the watchdog guarded no
live risk, yet cost 327 lines and 39 intrusive `set_stage` annotations spread
across the main loop. It also had a blind spot: synchronous `zbus` signal
emissions on the `StateRefresh` path ran under the restful `Idle` stage and
were never covered.

Stall attribution is retained through the existing `tracing` targets
(`typio.wayland.io`, `typio.engine.key`, `typio.config`, …): the last logged
target before a hang names the stage, with zero instrumentation cost.

## See Also

- [ADR-0041: Remove the host watchdog](../adr/0041-remove-watchdog.md) — the
  full rationale, audit table, and consequences.
- [Performance & Idle-Power Strategy](performance-strategy.md) — the
  idle-driven loop, now standalone (no watchdog liveness coupling).
- [Event Loop Scheduling](event-loop-scheduling.md) — the loop, now free of
  stage instrumentation.
