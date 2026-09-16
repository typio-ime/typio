# Subsystem Architecture: Daemon Lifecycle

- Status: Living Blueprint
- Last Updated: 2026-09-11
- Scope: core/daemon — the packaged service surface, the single poll loop, deadline folding, latency budgets, and stall containment
- Maintainers: Typio maintainers

---

## 1. System Overview & Boundaries

`typio` is a session daemon, not a foreground application: one instance per user session, resident for the whole graphical session, almost always in the background with no input focus. Its dominant cost is therefore idle behaviour rather than throughput — the design goal is that an idle daemon does nothing at all.

The subsystem covers three things:

1. **The packaged startup surface.** `data/typio.service.in` is the only packaged way the daemon starts. The unit is `Type=simple` with `ExecStart=@TYPIO_HOST_DIR@/typio`, `Restart=no`, `SyslogIdentifier=typio`, `StandardOutput=journal`, `StandardError=journal`, `PartOf=` / `After=graphical-session.target`, and `WantedBy=graphical-session.target`. There is no packaged `.desktop` launcher and no XDG autostart entry for the daemon; the only shipped desktop file is the settings application's (`data/applications/io.typio.Settings.desktop`). Logging goes to the user journal through the unit's stdout/stderr redirection: the daemon installs a `tracing` subscriber whose formatting layer writes to standard error (`crates/typio-daemon/src/diagnostics.rs`), colourised only when stderr is a terminal.
2. **The scheduling model.** One thread runs one `poll(2)` loop that multiplexes every source. Wayland dispatch happens first each step, and all auxiliary work runs after it in a bounded fashion.
3. **The latency and stall-containment budgets.** Every operation reachable from the loop must be non-blocking or bounded at its own call site, because there is no runtime safety net watching the loop.

The boundary stops before the subsystems that borrow the loop: input/session handling, Panel rendering, engine runtime, and the control plane each own their own blueprints. This blueprint owns *when and how often* they run, and the budgets that keep them from wedging the loop.

## 2. Invariants & Non-Negotiable Rules

- **`[INV-LOOP-01]` One loop owns the session.** Wayland objects and engine-session state are thread-affine and are only touched from the reactor thread. Auxiliary threads exist for bounded jobs only — the config file reader and the asynchronous engine respawn — and their results are installed by the loop on a later step.
- **`[INV-LOOP-02]` Wayland dispatch precedes auxiliary work.** Every iteration records facts and dispatches Wayland before any D-Bus, voice, config, or timer work, so input facts are fresh before non-input work can delay them.
- **`[INV-LOOP-03]` The idle baseline is an unbounded wait.** The baseline poll timeout is `-1` and every deadline lowers it through the earliest-deadline reducer. Introducing a periodic baseline tick to "keep the loop alive" is a defect.
- **`[INV-LOOP-04]` Every time-based wake is an explicit deadline or a self-waking fd.** Key repeat, the indicator timer, the voice-status timer, and the config-reload debounce own their own `timerfd` and join the poll set; anything else must contribute a deadline to the poll timeout.
- **`[INV-LOOP-05]` No runtime watchdog and no self-heal.** No background thread samples loop liveness and no stage is allowed to depend on such a sampler. Every stage must be non-blocking or bounded at its call site; a new blocking call must be caught in review, not at runtime.
- **`[INV-LOOP-06]` Hot-path engine IPC is bounded at 50 ms.** A `process-key` request carries the tight keystroke budget and a timeout must trigger poison recovery instead of a longer wait; `availability` uses its own 100 ms budget, and only cold-start and control-plane operations may use multi-second budgets.
- **`[INV-LOOP-07]` The config read cannot block the loop.** Config files are read on a short-lived reader thread with a bounded channel timeout; on timeout the load fails, the previous configuration is retained, and the loop continues.
- **`[INV-LOOP-08]` A poisoned engine worker is never reused.** Recovery discards the worker, respawns asynchronously on a detached thread, and passes keys through as unhandled while the respawn is in flight, so a multi-second spawn never runs on the keystroke path.
- **`[INV-LOOP-09]` Respawn attempts are rate-limited.** Consecutive failed recoveries double the wait (1, 2, 4, 8, 16, 32, 64 s, capped), the first retry after a crash is immediate, and the backoff resets after a successful respawned request or an explicit re-instantiation.
- **`[INV-LOOP-10]` Idle-paid work is throttled.** The idle-engine reaper walk runs at most once per 60 s and only stops workers inactive for at least 15 minutes; no per-keystroke path may take the engine-registry borrow.
- **`[INV-LOOP-11]` The systemd user unit stays the only packaged startup surface.** No desktop entry, autostart file, or second supervisor may be added for the daemon; restart and duplicate-start policy belong to the user service manager.
- **`[INV-LOOP-12]` Stall attribution is tracing-only.** The last logged target names the stalled stage; per-stage heartbeat instrumentation, stage enums, and `set_stage`-style annotations must not be reintroduced.

## 3. Component Architecture & Data Flow

### 3.1 Components

| Component | File | Responsibility |
| :--- | :--- | :--- |
| Service unit | `data/typio.service.in` | The packaged startup surface, journal logging, session ordering, restart policy |
| Loop and phase order | `crates/typio-daemon/src/app/event_loop.rs` | Fact bookkeeping, Wayland flush/read/dispatch, focus tick, key and repeat drivers, Panel convergence, status timers, voice, config, idle reaper |
| Reactor | `crates/typio-daemon/src/app/reactor.rs` | The nine named poll sources, the readiness snapshot, and `PollTimeout` earliest-deadline reduction |
| Input driver | `crates/typio-daemon/src/app/input_driver.rs` | Ordered key batches, engine output, virtual-keyboard forwarding, repeat |
| Panel driver | `crates/typio-daemon/src/app/panel_driver.rs` | Panel schedule convergence and presentation de-duplication |
| Event intake | `crates/typio-daemon/src/app/event_channel.rs` | Cross-thread typed daemon events and the wake eventfd drained at the loop head |
| Signals and resume | `crates/typio-daemon/src/app/signals.rs`, `crates/typio-daemon/src/resume_signal.rs` | Signal flags and suspend-gap facts |
| Engine backend | `crates/typio-runtime/src/core/engine/backend/process.rs` | Per-request timeouts, poison recovery, asynchronous respawn, backoff |
| Config loading | `crates/typio-runtime/src/config.rs` | Bounded reader-thread file load |
| Config watching | `crates/typio-daemon/src/config_watcher.rs` | inotify filter, debounce timer, watch rearm, reload boundary |
| Logging | `crates/typio-daemon/src/diagnostics.rs` | `tracing` subscriber, level control, signal-driven level raises |

### 3.2 The poll set

`PollSource` names every descriptor the loop watches; positional indexing is gone and the `-1`-means-unbounded rule lives in one tested type.

| Source | Kind | Notes |
| :--- | :--- | :--- |
| `Wayland` | socket | flush, prepare-read, dispatch; `POLLHUP`/`POLLERR` exits the loop |
| `Uds` | epoll fd | the control-plane bus, dispatched in the same step that observed readiness |
| `KeyRepeat` | timerfd | armed/disarmed explicitly; only a ready expiry drives a repeat |
| `ConfigWatch` | inotify | filtered to the config files |
| `ConfigTimer` | timerfd | config-reload debounce |
| `IndicatorTimer` | timerfd | indicator auto-hide |
| `VoiceTimer` | timerfd | voice status banner auto-hide |
| `VoiceSession` | eventfd | inference / async model-load completion |
| `ReactorWake` | eventfd | signals and cross-thread daemon events; takes priority over ordinary work |

`PollTimeout` starts with no deadline and reports `-1`; `reduce` keeps the earliest non-negative deadline and clamps every value at zero.

### 3.3 One reactor step

```text
drain cross-thread events (returns exit request)        0.
set connection_alive, sample the resume detector        1.
flush -> prepare_read -> dispatch pending Wayland      2.
poll(timeout = earliest deadline, or -1)               3.
read + dispatch Wayland; UDS dispatch; popup/grab/     4.
  commit response diagnostics (wayland_pending timeouts)
focus controller tick (reduce -> observe -> diff ->     5.
  apply), then indicator trigger for the transition
ordered key batch (engine, text, vk forwarding, repeat) 6.
repeat expiry                                         7.
Panel convergence (candidate flush)                   8.
positioned status flush (indicator / voice / anchor)  8b.
indicator auto-hide expiry                            9.
voice status auto-hide expiry                         10.
voice session dispatch and outcome drain              11.
config watch drain and debounced reload               12.
idle engine reaper (throttled)                        13.
```

### 3.4 Deadlines folded into the poll timeout

| Deadline | Active when | Source |
| :--- | :--- | :--- |
| Positioned-UI anchor probe | a popup request waits for a caret anchor | `PanelCoordinator::anchor_deadline_remaining_ms` |
| Wayland response diagnostics | commit, keymap, or probe response tracking is in flight | `wayland_pending.min_deadline_ms` |
| Bounded preedit coalescing | pure preedit waits for its 2 ms quiet or 4 ms hard deadline | `KeyboardRouter::preedit_deadline_remaining_ms` |
| Immediate re-dispatch | a dispatch in this step already produced queued work | reduced to 0 |

Everything else that is time-based owns a timerfd and needs no timeout. This is why the loop can block indefinitely while idle: no deadline is pending, and nothing needs to confirm the loop is alive.

### 3.5 Latency budgets

| Operation | Budget | Constant |
| :--- | :--- | :--- |
| Hot-path keystroke (`process-key`) | 50 ms | `ENGINE_KEY_TIMEOUT` |
| `availability` query | 100 ms | `ENGINE_REQUEST_TIMEOUT` |
| Cold start (`init`) | 60 s | `ENGINE_INIT_TIMEOUT` |
| Handshake before HELLO | 5 s | `ENGINE_HANDSHAKE_TIMEOUT` |
| `reload-config` | 5 s | `ENGINE_RELOAD_TIMEOUT` |
| `invoke-command` / control plane | 5 s | `ENGINE_COMMAND_TIMEOUT` |
| Voice (`process-audio`) | 120 s | `ENGINE_VOICE_TIMEOUT` |
| Other engine requests | 500 ms | `ENGINE_DEFAULT_TIMEOUT` |
| Config file read | 2 s on a reader thread, at most 4 reads in flight | `CONFIG_READ_TIMEOUT`, `MAX_CONFIG_READS_IN_FLIGHT` |

The budgets are selected per request by `request_timeout_for` in the engine backend, and the keystroke budget is what makes an unresponsive engine a bounded 50 ms loop pause rather than a stall.

### 3.6 Poison recovery

```text
process-key exceeds its budget / transport error
  -> worker is poisoned, never reused
  -> worker dropped, respawn + init on a detached thread
  -> while in flight the backend has no engine, so keys resolve as
     NOT_HANDLED and are forwarded to the application
  -> the next engine call installs the finished respawn
  -> on failure, the backoff deadline doubles (cap 64 s); on a successful
     respawned request, it resets
```

Only one respawn is ever in flight; an explicit re-instantiation (engine reload or registry re-register) supersedes an in-flight one and clears the backoff.

### 3.7 Stall containment and attribution

There is no watchdog. Containment is a property of the call sites:

- Wayland flush and event reads are non-blocking by construction and the loop polls before reading.
- The Panel path is a CPU canvas plus a non-blocking buffer attach, so presentation cannot block the loop.
- Engine IPC is bounded per operation as in the budget table.
- The one historically unbounded stage — a config file read on a possibly stalled network or FUSE mount — is now bounded by the reader-thread timeout.

Attribution relies on the `tracing` targets that already exist (`typio.wayland.io`, `typio.engine.key`, `typio.config`, `typio.lifecycle`, …): the last logged target before a hang names the stage, at zero instrumentation cost.

Two consequences are accepted rather than mitigated: a wedged (as opposed to crashed) daemon is no longer auto-terminated, and the packaged unit sets `Restart=no`, so a crash is not auto-restarted either. Recovery in those cases is manual — the compositor's VT-switch hotkeys bypass the IME grab, and the user service can be restarted from a TTY.

### 3.8 Retired mechanisms — do not reintroduce

| Retired | Why it must not return |
| :--- | :--- |
| The staged heartbeat watchdog (a background thread sampling per-stage liveness and `SIGKILL`ing the daemon) | Its risk had already been closed by other mechanisms; it cost a thread, a condition variable, an eleven-variant stage enum, and 39 call-site annotations, and it had a blind spot on synchronous D-Bus signal emission ([ADR-0041](../adr/0041-remove-watchdog.md)) |
| Per-stage `set_stage` / heartbeat annotations on every loop branch | The plain pipeline is the point; attribution comes from tracing targets ([ADR-0041](../adr/0041-remove-watchdog.md)) |
| A 100 ms baseline poll tick | It existed only to keep the watchdog heartbeat advancing; it costs idle wakeups for a resident daemon ([ADR-0024](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md)) |
| Demand-gated watchdog arming on focus transitions | Removed with the watchdog ([ADR-0037](../adr/0037-demand-armed-watchdog-cadence.md)) |
| A fixed per-frame heartbeat callback passed into Panel draw calls | It existed only to advance the watchdog during rendering ([ADR-0041](../adr/0041-remove-watchdog.md)) |
| A blocking config file read on the loop | The one genuinely unbounded stage; now bounded at the source ([ADR-0041](../adr/0041-remove-watchdog.md)) |

## 4. Historical Lineage & Founding ADRs

- Compaction status: **no record has been compacted yet.** All records below are active entries in [the ADR index](../adr/index.md); none has been tombstoned, relocated to `docs/adr/archive/`, or superseded by this blueprint.

| Record | What it established |
| :--- | :--- |
| [ADR-0004: Event-Loop Scheduling and Watchdog](../adr/0004-event-loop-scheduling-and-watchdog.md) | A single `poll()` loop multiplexing every source with an fd-plus-handler interface, deferred (not inline) UI rendering, debounced config reload, and the poll timeout as the deadline carrier (watchdog part superseded by ADR-0041) |
| [ADR-0009: Long-term performance optimizations](../adr/0009-long-term-performance-optimizations.md) | The resident-daemon principle that long sessions must not degrade: caches must be bounded and reclaimable, and per-keystroke work must short-circuit unchanged snapshots |
| [ADR-0021: systemd user service for daemon lifecycle](../adr/0021-systemd-user-service-daemon-lifecycle.md) | `typio.service` as the only packaged startup surface, stdout/stderr to the journal, no `.desktop` launcher or autostart entry, restart policy owned by the user service manager |
| [ADR-0024: Idle-Driven Event Loop and Demand-Gated Watchdog](../adr/0024-idle-driven-loop-and-demand-gated-watchdog.md) | The idle baseline becomes an unbounded poll, self-waking sources need no timeout, only three deadlines fold into the timeout, and the anchor-probe deadline becomes explicit |
| [ADR-0037: Demand-Armed Watchdog Cadence](../adr/0037-demand-armed-watchdog-cadence.md) | The watchdog starts disarmed, is armed by focus transitions, and samples coarsely while armed (superseded by ADR-0041) |
| [ADR-0041: Remove the host watchdog](../adr/0041-remove-watchdog.md) | The audit that every stage is non-blocking or bounded, removal of the watchdog and its annotations, the bounded config read, and stall attribution through tracing targets |
| [ADR-0048: Bounded Keystroke Latency Budget](../adr/0048-optics-lens-alignment-and-bounded-key-budget.md) | `ENGINE_KEY_TIMEOUT` of 50 ms for the hot keystroke path, with poison recovery and asynchronous respawn as the response to exceeding it |
| [ADR-0049: TOML-only configuration](../adr/0049-toml-only-configuration.md) | A single configuration dialect: a malformed file is a parse error, and the last known-good configuration is retained instead of a silently partial tree |

## See Also

- [Event Loop Scheduling](../explanation/event-loop-scheduling.md) — phase ordering, Panel bounds, and poll management
- [Performance & Idle-Power Strategy](../explanation/performance-strategy.md) — why wakeups, not CPU cycles, are the metric
- [Stall Containment](../explanation/stall-containment.md) — what the removed watchdog guarded, and why the host now depends on bounded stages instead
- [Input Session](input-session.md) — the focus pipeline the loop drives each step
- [Panel Rendering](panel-rendering.md) — the bounded render and present path the loop flushes
