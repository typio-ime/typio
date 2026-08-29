# Wayland Input Method Protocol

`typio` is a Wayland-native input method. Every key the user presses, every preedit string shown, and every positioned Panel update travels through the `zwp_input_method_v2` family of unstable protocols. This document maps how the daemon implements those protocols, what workarounds it applies to the unstable surface, and where the detailed rules live.

This is a **connective-tissue** document: it does not replace the protocol specification, the source-code comments, or the deep-dive timing model. It exists so a reader can answer "how does typio handle X?" in one stop rather than grepping across `input_method.c`, `keyboard.c`, and the input helper modules.

For the protocol specification see the upstream [wayland-protocols `input-method-unstable-v2.xml`](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/unstable/input-method/input-method-unstable-v2.xml). For session lifecycle, build-up chain, and daemon-resilience rules see [Input-Method Session](input-method-session.md). For event-loop scheduling and GPU bounds see [Event Loop Scheduling](event-loop-scheduling.md).

## Protocol Stack

The daemon binds five Wayland protocol layers. The input-method layer is the one the daemon *implements*; the others are dependencies or peers.

### Compositor-provided interfaces (daemon is the consumer)

| Interface | How the daemon uses it |
|---|---|
| `zwp_input_method_manager_v2` | `get_input_method()` → receives the `zwp_input_method_v2` object the daemon listens on |
| `zwp_input_method_keyboard_grab_v2` | `grab_keyboard()` → receives raw key/modifier/keymap events for the focused input context |
| `zwp_input_popup_surface_v2` | `get_input_popup_surface()` → positions the Panel near the cursor |
| `zwp_text_input_manager_v3` | The daemon does not bind this directly, but relies on the compositor exposing it so client applications can participate in the text-input session |
| `wl_compositor` | `create_surface()` → creates the Panel `wl_surface` used for SHM buffer attaches |
| `wl_surface` preferred scale | Tracks compositor scale hints so the Panel renders at the correct DPI for each monitor |
| `wl_shm` | Creates host-managed buffers for the offscreen-rendered Panel |
| `wp_viewporter` | Crops a quantized, hysteretically sized CPU framebuffer to the exact logical Panel size |

### Client-provided interfaces (daemon depends on their presence)

| Interface | Role |
|---|---|
| `zwp_text_input_v3` | The application side (Firefox, terminal, chat) enables text input, sends surrounding text and content type to the compositor, which forwards it to the daemon through `zwp_input_method_v2` |

### Daemon-provided interfaces

| Interface | Role |
|---|---|
| `zwp_input_method_v2` | Listens for `activate`, `deactivate`, `done`, `surrounding_text`, `text_change_cause`, `content_type`, `unavailable`; sends `set_preedit_string`, `commit_string`, `commit`, `delete_surrounding_text` |
| `zwp_virtual_keyboard_v1` | Forwards unhandled keys back to the compositor as synthetic press/release events; managed by the virtual-keyboard bridge for keymap handoff and readiness gating |

## Event Handlers: events become facts

Every `zwp_input_method_v2` event does one thing — **record a fact** into
`frontend->focus_facts`. Focus facts are classified at `done`; resource
drift is checked by the focus controller on the event-loop path. This is
what keeps protocol handlers small and the lifecycle boundaries explicit.

Facts are consumed, not stored, per the focus controller model: each reactor
step clears the fact buffer, events refill it during dispatch, and `reduce()`
derives the desired state atomically at the end of the batch. See [Focus
Controller](focus-controller.md).

### `activate` / `deactivate`

Record the pending focus fact (`active = true` / `false`) for the current session, creating the session if none exists. `activate` additionally records an `activate_seen` fact for the current `done` batch and hides any stale positioned indicator from the prior activation. They make no transition decision themselves; that happens at `done`. `activate_seen` is what lets `done` tell a genuine (re)activation apart from a plain text-state update; see [ADR-0018](../adr/0018-focus-transition-classification.md).

### `surrounding_text`, `text_change_cause`, `content_type`

Record client editing-context facts (buffered during the `done` batch). These are hints, not commands; the engine may ignore them or use them to improve prediction.

### `done`

The compositor's double-buffer commit point, and where focus facts become lifecycle actions.

**Why double-buffering?** The `zwp_input_method_v2` protocol sends a batch of events (`activate`, `deactivate`, `surrounding_text`, `content_type`, …) followed by a single `done`. Events in the batch are provisional — they record *facts* into a pending buffer, but no action is taken until `done` commits the batch atomically. This is the same pattern as `wl_surface::commit`: stage changes, then apply them all at once.

**Why not react per-event?** Two scenarios demonstrate the problem:

1. **Cancelled activation.** A UI flicker can produce `activate` → `deactivate` within one batch. Per-event handling would build the keyboard grab, call engine `focus_in`, show the indicator — then immediately tear it all down. With `done`-time reduction, the two facts cancel out: `was=false, now=false` → `NONE`, zero work done.

2. **Reactivation.** Clicking from one text field to another inside the same window produces `activate` while already active (no intervening `deactivate`). Per-event handling would build a new grab on `activate`, destroying the existing one mid-composition. With `done`-time reduction: `was=true, now=true, activate_seen=true` → `REACTIVATE` — the grab and composition are preserved, old-field key/repeat state is fenced, and the Panel is re-anchored and re-presented.

**Steps at `done`:**

1. **Serial increment**. `im_serial++`. The serial is the count of `done` events received; it is the commit serial for every `zwp_input_method_v2_commit()` call.
2. **Apply facts**. The buffered `surrounding_text`, `content_type`, `text_change_cause`, and `active` facts become current atomically.
3. **Classify the state change.** The focus facts are reduced by the pure `focus_controller::reduce` into a desired state with edge-triggered focus-in, focus-out, and reactivation flags, which the per-iteration pipeline in `crates/typio-host/src/app/event_loop.rs` consumes through `diff` effects. Diff converges every iteration, so a no-op tick that still finds a non-routable grab recovers naturally on the next pass; there is no separate reconciler. See [ADR-0018](../adr/0018-focus-transition-classification.md) and [ADR-0003](../adr/0003-session-controller-reduce-diff.md).

### `unavailable`

Another input method has taken the seat. The daemon sets `frontend->running = false`, logs a warning, and stops.

## Commit Serial and Text Transactions

`zwp_input_method_v2` requires a serial on every `commit()` call. The serial must match the most recent `done` event known to the daemon. Before the first `done`, the serial is 0.

The daemon treats serial 0 as a **write barrier**: `InputMethodState::text_transaction_and_flush()` and `commit_protocol_state()` refuse to send protocol commits before the compositor has established the input-method connection. This prevents a race where Typio stages preedit text before the compositor can apply it, which would cause the compositor to silently drop the staged text without error.

Text payloads use one explicit transaction entry point:

```rust
InputMethodState::text_transaction_and_flush(
    commit_text: Option<&str>,
    preedit: Option<(&str, u32)>,
);
```

The helper stages `commit_string` and/or `set_preedit_string`, then sends one `commit(serial)`.  It is the only path for text payload commits.  Non-text lifecycle/focus state uses `commit_protocol_state()` so code reviewers can see that no preedit or commit string is being sent.

The keyboard router owns the staging boundary. It updates candidate state immediately, but stages composition-only preedit in a bounded latest-wins coalescer. The value becomes eligible after a 2 ms quiet period and cannot wait longer than 4 ms from the first update in a burst. If an engine emits real commit text, the router flushes immediately so commit order remains strict. If one key produces both commit text and a replacement preedit, both are sent in the same Wayland transaction.

This is deliberate: two fast key events can be delivered in separate reactor steps before Typio reads the compositor's next `done`. Submitting every intermediate preedit as its own `commit(serial)` can create same-serial commits where later values become stale. See [ADR-0042](../adr/0042-text-input-transaction-staging.md) and [ADR-0043](../adr/0043-bounded-preedit-coalescing.md). The host does not hold preedit for compositor `done`; that event is a compositor state boundary, not a text-commit ack.

## Keyboard Grab Lifecycle

The grab and its keymap handshake are **one resource** (`absent → needs_keymap → ready`) that the focus controller creates and repairs on each reactor evaluation; the rules are in [Input-Method Session](input-method-session.md).

Briefly:
- Each grab incarnation has a **generation**. A key press claims the current generation, and the matching release is accepted only when the stored per-key generation still matches the active grab generation.
- When a grab is rebuilt (focus-in, resume, reconnect), the compositor may
  re-send already-in-flight keys; the generation fence discards them.
- Re-activation retains the grab but synthesizes releases for keys forwarded
  by the old field and stops their repeat chain before routing in the new
  field.
- Keys queued but not yet routed when an `activate`/`deactivate` boundary
  arrives are discarded wholesale. An unrouted key belongs to the activation
  epoch it arrived in; routing it after the boundary would inject it into
  whatever field is focused next (the "wwwwww" regression, where a
  shortcut's letter key — Ctrl+W closing a browser tab — was routed into the
  newly focused field and its armed repeat kept firing).
- An armed repeat chain is stopped at every focus transition (focus-out,
  destroy-grab, focus-in, reactivate), and each expiration is re-validated
  against the key's tracking state and the current modifier mask: a key
  whose release was synthesized at a boundary, or a blocking-modifier
  (Ctrl/Alt/Super) transition since the chain was armed, ends the chain
  before any key is emitted.
- Unhandled keys are forwarded as original press/release pairs through the virtual keyboard.
- Modifier state is synced separately; modifier changes do not synthesise releases for unrelated non-modifier keys.
- Wedged-grab recovery relies on external fact sources (resume detector, POLLHUP) — see [Focus Controller](focus-controller.md). The C host's emergency-exit shortcut and rejected-press-streak failsafe were not ported.

## Engine Availability and Fault Isolation

The daemon must remain responsive even when third-party engine processes are buggy, slow to initialize, or fail entirely. This section describes the patterns that prevent engine failures from bringing down the input method.

### Bounded initialization and availability queries

Engine discovery reads `EngineHello` before heavyweight initialization. The
runtime then sends `init` over the private engine channel with a bounded
60-second cold-start budget, so loading dictionaries or deploying schemas can
never block indefinitely.

Once initialized, the router queries the active engine with the typed
`availability` request. That hot-path request has a 100-millisecond deadline.
`PREPARING` keeps keys inside the input method without forwarding incomplete
state; `READY` enables normal routing; transport failure becomes `FAILED` and
poisons the worker channel for supervised restart. No engine calls into the
daemon and no raw callback crosses a thread or process boundary.

### Engine Process Isolation

Every engine runs out of process. The daemon sends lifecycle, key, mode,
candidate, availability, and voice requests over Typio Engine Protocol on the
private engine fd. If an engine process crashes, the daemon observes a
transport failure instead of taking the fault in the Wayland process. Engine
code runs inside direct engine executables, not inside the daemon.

A poisoned worker is respawned **asynchronously**: the respawn + re-init runs
on a detached thread and is installed by the next engine call. While a
recovery is in flight the backend reports no engine, so keyboard keys pass
through to the application (`NOT_HANDLED`) instead of freezing the main loop
on a multi-second spawn while the keyboard grab is held.

See the [engine contract](../../crates/typio-core/docs/explanation/engine-contract.md)
for process isolation, ownership, and poisoned-channel recovery.

## Virtual Keyboard Forwarding

`zwp_virtual_keyboard_v1` is the daemon's output path for keys the engine
declined with `KeyResult::NotHandled`. The virtual-keyboard bridge manages:

- **Keymap handoff** — when the compositor delivers a new keymap on the grab, the vk must receive the same keymap before forwarding keys, or modifier mappings will mismatch.
- **Readiness gating** — vk forwarding is blocked until the keymap is confirmed, preventing modifier-sync errors during activation handshakes.
- **Fail-safe downgrade** — if vk health degrades (keymap deadline missed, compositor stalls), the daemon falls back to local key handling rather than forwarding broken state.

## Re-activate while focused: re-anchor, keep the grab

A subtle protocol behaviour: the compositor may send `activate` while the daemon is still focused (e.g. the user clicked from one text field to another inside the same window). Treating this as a full `deactivate` → `activate` cycle would tear down the grab, lose the preedit round-trip, and interrupt typing.

The `activate_seen` fact makes this case explicit without that cost. When a
`done` batch carried an `activate` while the previous tick was already `YES`,
`reduce` sets `reactivate` (`focus_controller.rs`). The keyboard grab and
the engine's input context are **left intact** because they belong to the
input method, not the field. Transient key ownership does belong to the old
field, so the daemon synthesizes releases for forwarded keys and stops the
repeat timer. It then resets the Panel anchor, invalidates the submitted-frame
cache, and re-presents a non-empty candidate snapshot even when its sequence
number did not change. A `done` with no `activate` this batch keeps focus
state untouched, so plain
text-state updates during composition never disturb the grab. See
[ADR-0018](../adr/0018-focus-transition-classification.md).

## Resume and silent grab loss

System suspend is invisible to the Wayland protocol: no `deactivate` before sleep, no guaranteed `activate` after wake; a held modifier may be stuck and the grab may be silently dead on wake. The compositor can also drop the grab with no event at all (restart, bug, race).

How much of this the focus controller handles depends on whether
`session_glue::observe()` can *see* it — and observation reads resource
*presence*, not *liveness*:

- **Suspend.** A grab dead across suspend leaves a *live proxy*; observation reports it healthy, so the focus controller alone is blind. A resume **detector** (logind `PrepareForSleep` plus a `CLOCK_BOOTTIME` vs `CLOCK_MONOTONIC` gap heuristic) records facts: it invalidates the grab generation and drops the compositor-visible preedit, then lets the next reactor step rebuild as needed.
- **Grab object gone.** If the grab *object* is actually absent while `desired.grab` is still `YES`, observation reports `ABSENT` and the diff recreates it.

In both cases the input context is never `focus_out`'d, so the engine's in-flight composition survives, and the rebuild is the *same* grab build used on first focus.

## Indicator behaviour

The indicator (the transient Panel showing the active engine and mode label) has three show paths, each with different gate semantics:

| Path | Trigger | Gates |
|---|---|---|
| First-focus (`show_on_focus`) | `FirstActivate` only | salience (suppress `QUIET` states) + acknowledged-recency (suppress if user typed or saw indicator within the last 3 s) |
| Reactivate (`show_on_reactivate`) | `Reactivate` | salience only — the user moved to a new caret in the same session, so do not suppress on recency (the new field's context can differ from the previous one's) |
| Deliberate-change (`show_for_state_change`) | engine switch, mode change, profile toggle, `summon_indicator` shortcut | none — user just acted, always announce |

On `DEACTIVATE`, the indicator is hidden along with all other Panel UI. On `REACTIVATE`, the indicator re-evaluates against the salience gate: `NOTABLE` modes re-show (the user has moved to a new caret whose context can differ), while `QUIET` modes stay suppressed. The recency gate does not apply on `REACTIVATE`. See [ADR-0018](../adr/0018-focus-transition-classification.md).

The indicator auto-hides after `display.indicator_duration_ms` (default 1500 ms, clamped 100–10000 ms) via a timerfd.

## Known limits: terminal multiplexers

Terminal emulators (foot, Alacritty, kitty, …) register the **entire terminal window** as a single `zwp_input_method_v2` input area. When a terminal multiplexer like tmux or screen splits that window into panes, sessions, or windows, those context switches happen entirely inside the terminal's own rendering — they produce **no Wayland focus events**. From the compositor's perspective, the user never left the same input field.

Consequences for indicator behaviour:

- Switching tmux panes or windows does **not** trigger `activate`/`deactivate`, so the daemon never sees a `REACTIVATE`. Any indicator that was showing stays visible until its auto-hide timer expires.
- There is no way for the input method to detect intra-terminal context switches. This is an inherent limitation of the `zwp_input_method_v2` protocol model, which only knows about compositor-level focus, not application-level editing context.
- The auto-hide timer (`indicator_duration_ms`) is the only mechanism that dismisses the indicator in this scenario. Lowering the duration makes the indicator vanish sooner but shortens the display window for all contexts, including those where the indicator is useful.

This limitation also affects the candidate Panel position: the input-popup surface is anchored to the terminal's cursor rectangle, not to a tmux pane boundary.

## Preedit Transaction Optimisation

When the user navigates candidates with `Up`/`Down`, only the `selected` index changes; the preedit text is identical. The daemon detects this with `text_ui_plan_update` against the last compositor-facing preedit, including any preedit already staged for the current key drain:

```rust
let plan = text_ui_plan_update(last_text, last_cursor, new_text, cursor_pos);
```

If `plan == TextUiPlan::SyncPanelOnly`, the Panel Scheduler refreshes only the Candidate Panel during the event-loop Panel stage. The expensive `zwp_input_method_v2.set_preedit_string` → `commit(serial)` → `done` round-trip to the application is skipped entirely.

If the preedit did change, the router stages the new preedit rather than committing it immediately. Composition-only bursts coalesce across adjacent reactor steps to the latest staged value, subject to the 4 ms hard limit; commit-producing keys flush immediately after the matching composition is drained so text order stays correct. This avoids both redundant round-trips and stale same-serial commits in heavyweight clients and compositors.

## Source Map

| Protocol object | Source file | Responsibility |
|---|---|---|
| `zwp_input_method_v2` | `crates/typio-host-platform/src/input_method.rs` | Event handlers (record facts), text transaction entry point, lifecycle protocol commits |
| Focus controller (pure) | `crates/typio-host/src/focus_controller.rs` | `reduce` / `diff` / guard predicates — dependency-free, unit-tested |
| Session effects (effectful) | `crates/typio-host/src/session_glue.rs` | `observe` and `apply`, including hard teardown and effect ordering |
| `zwp_input_method_keyboard_grab_v2` | `crates/typio-host-platform/src/input_method.rs` (`Dispatch<ZwpInputMethodKeyboardGrabV2>`) | Grab create/destroy, key/modifiers/repeat listeners, keymap handoff to vk |
| Key generation + tracking | `crates/typio-host/src/keyboard_policy.rs`, `crates/typio-host/src/keyboard/router.rs` | Generation fence and symmetric press/release |
| Preedit coalescing | `crates/typio-host/src/keyboard/preedit_coalescer.rs` | Latest-wins 2 ms quiet window with 4 ms hard limit |
| Reactor coordination | `crates/typio-host/src/app/event_loop.rs`, `crates/typio-host/src/app/reactor.rs`, `crates/typio-host/src/app/input_driver.rs` | Named readiness sources, deadline reduction, ordered keyboard/text phases |
| `zwp_virtual_keyboard_v1` | `crates/typio-host-platform/src/input_method.rs` (`forward_key`, `forward_modifiers`) | Keymap forward, modifier mirror, unhandled-key forwarding |
| `zwp_input_popup_surface_v2` | `crates/typio-host-platform/src/input_method.rs`, `crates/typio-host-platform/src/panel.rs` | Panel positioning and SHM commits |
| Panel rendering | `crates/typio-host-platform/src/panel.rs`, `crates/typio-host-platform/src/panel_shm.rs` | CPU canvas render + `TextRaster` glyph composite, host-managed SHM attach |
| Resume detection | `crates/typio-host/src/resume_signal.rs` | logind + boottime heuristic (records facts) |
| Protocol XML | `protocols/input-method-unstable-v2.xml` | Wayland protocol definition (upstream) |

## See Also

- [Input-Method Session](input-method-session.md) — declared lifecycle phase, observed axes, key generation fencing, and daemon resilience (suspend/resume, compositor restart, silent grab loss)
- [Event Loop Scheduling](event-loop-scheduling.md) — event-loop scheduling, GPU bounds, D-Bus dispatch, and poll deadlines
- [ADR-0042: Text-input transaction staging](../adr/0042-text-input-transaction-staging.md) — why preedit commits are staged/coalesced at key-drain boundaries
- [ADR-0043: Bounded preedit coalescing](../adr/0043-bounded-preedit-coalescing.md) — how adjacent cross-step keys coalesce without waiting on `done`
- [Panel Appearance](../dev/panel-appearance.md) — offscreen Panel rendering pipeline
