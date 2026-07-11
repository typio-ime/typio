# Performance & Idle-Power Strategy

`typio` is a resident daemon: it is launched at session start and runs for the
whole session, almost always in the background with no input focus. Its
dominant cost is therefore not throughput but **idle behaviour** — what it does
during the long stretches when the user is not typing. The guiding rule:

> When there is no work, the process should do *nothing* — no polling, no
> periodic wakeups, no timers firing "just in case".

The enemy of idle power is **wakeups**, not CPU cycles. A modern CPU saves
power by dropping into deep idle (C-)states; every timer wakeup pulls it back
out for a fixed energy cost. A daemon that wakes 10–20 times a second while
idle is "death by a thousand cuts" across a session — and it is exactly what
`powertop` lists per process.

## Event-driven, not polled

The frontend multiplexes every source in one `poll()` loop
([ADR-0004](../adr/0004-event-loop-scheduling-and-watchdog.md)). The default
poll timeout is **`-1` (block indefinitely)**: the loop sleeps until the kernel
makes a file descriptor ready. Idle wakeups from the loop are therefore **zero**
([ADR-0024](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md)).

Anything time-based is expressed as a **deadline**, never a busy tick. There are
two kinds:

### Self-waking sources (`timerfd` in the poll set)

Key repeat, the indicator timer, and the config-reload debounce each own a
`timerfd` registered in the poll set. They wake the loop precisely when they
fire and cost nothing until then — no poll timeout is needed for them.

### Timeout-driven deadlines (fold into the poll timeout)

Three deadlines are not backed by an fd and so must bound the poll timeout.
Each lowers it through the `PollTimeout` earliest-deadline reducer, leaving the
timeout infinite when none is pending:

| Deadline | When active | Source |
|----------|-------------|--------|
| Bounded preedit coalescing | pure preedit is waiting for its 2 ms quiet or 4 ms hard deadline | [ADR-0043](../adr/0043-bounded-preedit-coalescing.md) |
| Positioned-UI anchor probe | a popup waits for a caret anchor that may never re-arrive | [ADR-0017](../adr/0017-positioned-ui-arbitration.md) |
| Wayland response diagnostics | commit, keymap, or probe response tracking is active | grab/text/popup protocol chains |

The anchor-probe deadline previously relied on a 100 ms baseline tick to be
re-evaluated; it is now folded in explicitly, so removing the tick does not
strand a pending popup.

## Why blocking indefinitely at idle is safe

A liveness watchdog would naively need a periodic tick to confirm the loop is
alive — and an earlier design did exactly that, which is *why* the loop once
ticked at 100 ms. That coupling no longer exists: the watchdog was removed
([ADR-0041](../adr/0041-remove-watchdog.md)) after an audit showed every
main-loop stage is non-blocking or bounded (engine IPC 100 ms, Wayland I/O
non-blocking, config-read 2 s). The loop simply blocks on `poll()` until work
arrives; nothing needs to confirm its liveness, because nothing can wedge it.

## Filesystem watching is push, not poll

Config changes are detected with `inotify` on the config directory, not by
stat-polling. The watch is filtered to `core.toml` / `platform.toml` so editor
swap/backup files and unrelated directory churn do not trigger work; relevant
events are debounced (100 ms) and coalesced into one reload. Idle cost is
nil — the kernel pushes events.

## Other strategies (throughput, not idle)

Idle power is the headline, but the same "do work once, lazily" instinct runs
through the hot paths, documented elsewhere:

- Candidate-popup rendering is **flushed once per reactor step**, so a burst
  of keystrokes collapses to one render
  ([ADR-0004](../adr/0004-event-loop-scheduling-and-watchdog.md),
  [ADR-0023](../adr/0023-panel-scheduler-state-machine.md)).
- Text measurement is cached by content, font, and scale; rasterisation writes
  directly into the CPU canvas only when the candidate snapshot needs a new
  frame.
- The composition pipeline short-circuits and fast-paths unchanged snapshots
  ([ADR-0009](../adr/0009-long-term-performance-optimizations.md)).

## Measuring

Idle wakeups are the metric. With the daemon running and **no input focus**:

```sh
sudo powertop        # Overview / "Frequency stats" tab → find the `typio` row
```

Expect the per-process wakeups/s to sit at ≈ 0 when idle, rising only while a
field is focused or a deadline above is pending. `perf stat -e sched:sched_switch`
or `/proc/<pid>/status` (`voluntary_ctxt_switches`) are coarser cross-checks.

## See Also

- [ADR-0024: Idle-Driven Event Loop and Demand-Gated Watchdog](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md)
- [Event Loop Scheduling](event-loop-scheduling.md)
- [Configuration Reference](../reference/configuration.md) — config-watch reload behaviour.
