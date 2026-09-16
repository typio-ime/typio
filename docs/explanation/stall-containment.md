# Stall Containment

Typio deliberately has **no runtime self-heal**. Nothing inside the daemon
samples the main loop's progress, kills the process when a stage wedges, or
restarts it. Containment is instead a property of every call site: each stage
reachable from the loop is non-blocking or bounded, so no stage can hold the
loop open indefinitely. When something does hang, the stage is named by ordinary
logging rather than by dedicated liveness instrumentation.

## Why There Is No Runtime Self-Heal

The daemon once carried a watchdog: a background thread that sampled the main
loop's progress at a named stage and killed the daemon when a stage stalled.
[ADR-0004](../adr/0004-event-loop-scheduling-and-watchdog.md) introduced it on
the theory that a grab-holding input method that wedges locks the user out of
all typing — including out of any terminal they would open to kill it — and that
an independent thread detecting the stall and killing the process, with the
systemd user service restarting the daemon, was the one self-heal worth having
for that worst case.

An audit of the main loop's blocking points established that every risk the
watchdog guarded had already been closed by other mechanisms, so it was removed
([ADR-0041](../adr/0041-remove-watchdog.md)):

- Engine IPC stalls — the strongest historical motivator — are bounded by
  per-operation deadlines on the engine channel.
- GPU present deadlocks were eliminated when the Panel moved to CPU rendering
  plus non-blocking shared-memory attach
  ([ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md)).
- Wayland I/O is non-blocking by construction.
- Config reload — the one genuinely unbounded blocking point, a file read on a
  possibly stalled network or FUSE mount — is now bounded at the source.

The watchdog therefore guarded no live risk, while costing a thread, a condition
variable, a stage enumeration, and dozens of annotations spread across the main
loop. It also had a blind spot: synchronous D-Bus signal emission on the
state-refresh path ran under the stage that the watchdog treated as restful
idleness, so a stall there would never have been attributed.

## Every Stage Is Bounded at Its Call Site

The audit's conclusion is the design rule in force today: because no sampler
watches the loop, every stage must bound itself. A new blocking call is a
review-blocking defect, not something to catch at runtime.

| Stage | How it is bounded |
|-------|-------------------|
| Hot-path engine IPC | A keystroke request carries a 50 ms deadline, and an availability query 100 ms; exceeding a deadline poisons the worker and triggers asynchronous respawn instead of a longer wait |
| Control-plane engine operations | Explicit budgets an order of magnitude larger than the keystroke path, and still finite: cold start, config reload, command invocation, and voice processing each have their own |
| Wayland I/O | Flush, event reads, and dispatch are non-blocking by construction, and the loop polls for readability before it reads |
| Panel presentation | The Panel renders on the CPU and attaches a shared-memory buffer without blocking; when every buffer is busy the frame is dropped and retried on a later reactor step |
| Config reload | The file read runs on a short-lived reader thread with a bounded channel wait; on timeout the load fails, the previous configuration is retained, and the loop continues |
| D-Bus and other auxiliary sources | Callbacks enqueue typed events instead of running on the loop; the loop drains a bounded amount of auxiliary work per step, and every time-based source owns its own timer descriptor |

The engine-respawn path follows the same discipline: recovery discards the
poisoned worker and respawns on a detached thread, while keys pass through to the
application during the respawn window. A multi-second engine start therefore
never runs on the keystroke path.

Because every stage is bounded, an unresponsive peer becomes a bounded pause
rather than a stall: the loop resumes on its own once the deadline expires, and
no external actor has to intervene.

## Attribution Through Logging

Stall attribution is retained through the logging that already exists. Each
subsystem logs under its own target namespace — Wayland I/O, engine key
handling, configuration, daemon lifecycle, and so on — so the last target logged
before a hang names the stage that was executing, at zero instrumentation cost.
No new code is required to attribute a stall; the ordinary operational log is
already the evidence.

Per-stage heartbeat instrumentation, stage enumerations, and stage-annotation
calls on loop branches must not be reintroduced: the plain pipeline is the point
([ADR-0041](../adr/0041-remove-watchdog.md)). For the same reason there is no
baseline poll tick; the loop blocks indefinitely when idle, and only real
deadlines shorten the wait
([ADR-0024](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md)). See
[Event Loop Scheduling](event-loop-scheduling.md) for those deadlines, and the
[Daemon Lifecycle Blueprint](../architecture/daemon-lifecycle.md) for the exact
targets and per-operation budgets in force today.

## Accepted Consequences

Two consequences are accepted rather than mitigated:

- A **wedged** daemon — as opposed to a crashed one — is no longer
  auto-terminated.
- The packaged user service does not auto-restart a crash either; restart policy
  belongs to the user service manager
  ([ADR-0021](../adr/0021-systemd-user-service-daemon-lifecycle.md)).

Recovery in those cases is manual, and it is reachable: the compositor's
virtual-terminal switch hotkeys bypass the input-method grab, so the user can
still reach a terminal and restart the user service from there. This is the
trade the project made — a permanent background thread and its annotations
against a manual, rare recovery path.

## See Also

- [ADR-0041: Remove the host watchdog](../adr/0041-remove-watchdog.md) — the
  full rationale, the audit table, and the consequences.
- [ADR-0004: Event Loop Scheduling and Watchdog](../adr/0004-event-loop-scheduling-and-watchdog.md)
  — the loop and watchdog the project started from.
- [ADR-0024: Idle-Driven Event Loop and Demand-Gated Watchdog](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md)
  — the idle baseline that removed the fixed poll tick.
- [Performance & Idle-Power Strategy](performance-strategy.md) — the
  idle-driven loop, now standalone (no watchdog liveness coupling).
- [Event Loop Scheduling](event-loop-scheduling.md) — the loop, now free of
  stage instrumentation.
