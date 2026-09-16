# Wayland Input Method Protocol

`typio` is a Wayland-native input method. Every key the user presses, every preedit string shown, and every positioned Panel update travels through the `zwp_input_method_v2` family of unstable protocols. This document maps how the daemon implements those protocols, what workarounds it applies to the unstable surface, and where the detailed rules live.

This is a **connective-tissue** document: it does not replace the protocol specification, the source-code comments, or the deep-dive timing model. It exists so a reader can answer "how does typio handle X?" in one stop rather than searching the protocol bindings in the platform layer, the key-routing modules, and the input helper modules.

For the protocol specification see the upstream [wayland-protocols `input-method-unstable-v2.xml`](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/unstable/input-method/input-method-unstable-v2.xml). For session lifecycle, build-up chain, and daemon-resilience rules see [Input-Method Session](input-method-session.md). For event-loop scheduling and poll deadlines see [Event Loop Scheduling](event-loop-scheduling.md).

## Protocol Stack

The daemon binds five Wayland protocol layers. The input-method layer is the one the daemon *implements*; the others are dependencies or peers.

### Compositor-provided interfaces (daemon is the consumer)

| Interface | How the daemon uses it |
|---|---|
| `zwp_input_method_manager_v2` | `get_input_method()` → receives the `zwp_input_method_v2` object the daemon listens on |
| `zwp_input_method_keyboard_grab_v2` | `grab_keyboard()` → receives raw key/modifier/keymap events for the focused input context |
| `zwp_input_popup_surface_v2` | `get_input_popup_surface()` → positions the Panel near the cursor |
| `zwp_text_input_manager_v3` | The daemon does not bind this directly, but relies on the compositor exposing it so client applications can participate in the text-input session |
| `wl_compositor` | `create_surface()` → creates the Panel surface used for shared-memory buffer attaches |
| `wl_surface` preferred scale | Tracks compositor scale hints so the Panel renders at the correct DPI for each monitor |
| `wl_shm` | Creates host-managed shared-memory buffers for the CPU-rendered Panel |
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

Every `zwp_input_method_v2` event does one thing — **record a fact** into the session's pending fact buffer. Focus facts are classified at the protocol commit boundary; resource drift is checked by the focus controller on the event-loop path. This is what keeps protocol handlers small and the lifecycle boundaries explicit.

Facts are consumed, not stored, per the focus controller model: each reactor step clears the fact buffer, events refill it during dispatch, and the desired state is derived atomically at the end of the batch. See [Focus Controller](focus-controller.md).

### `activate` / `deactivate`

Record the pending focus fact (active or not) for the current session, creating the session if none exists. An activation additionally records that an activation was seen in the current commit batch and hides any stale positioned indicator from the prior activation. Neither handler makes a transition decision; that happens at protocol commit time. The recorded activation fact is what lets the commit step tell a genuine (re)activation apart from a plain text-state update; see [ADR-0018](../adr/0018-focus-transition-classification.md).

### `surrounding_text`, `text_change_cause`, `content_type`

Record client editing-context facts (buffered during the commit batch). These are hints, not commands; the engine may ignore them or use them to improve prediction.

### `done`

The compositor's double-buffer commit point, and where focus facts become lifecycle actions.

**Why double-buffering?** The input-method protocol sends a batch of events (`activate`, `deactivate`, `surrounding_text`, `content_type`, …) followed by a single commit event. Events in the batch are provisional — they record *facts* into a pending buffer, but no action is taken until that commit applies the batch atomically. This is the same pattern as a surface commit: stage changes, then apply them all at once.

**Why not react per-event?** Two scenarios demonstrate the problem:

1. **Cancelled activation.** A UI flicker can produce `activate` → `deactivate` within one batch. Per-event handling would build the keyboard grab, focus the engine, show the indicator — then immediately tear it all down. With commit-time reduction, the two facts cancel out: the session was inactive and is still inactive, so the transition is a no-op and no work is done.

2. **Reactivation.** Clicking from one text field to another inside the same window produces `activate` while already active (no intervening `deactivate`). Per-event handling would build a new grab on `activate`, destroying the existing one mid-composition. With commit-time reduction, the batch is recognised as a reactivation: the grab and composition are preserved, old-field key/repeat state is fenced, and the Panel is re-anchored and re-presented.

**Steps at the commit boundary:**

1. **Serial increment.** The daemon advances its serial, the count of protocol commits it has received; that value is the commit serial for every subsequent protocol request.
2. **Apply facts.** The buffered surrounding-text, content-type, text-change-cause, and activation facts become current atomically.
3. **Classify the state change.** The focus facts are reduced to a desired state with edge-triggered focus-in, focus-out, and reactivation flags, which the per-iteration pipeline consumes as a diff of effects. Diff converges every iteration, so a no-op tick that still finds a non-routable grab recovers naturally on the next pass; there is no separate reconciler. See [ADR-0018](../adr/0018-focus-transition-classification.md) and [ADR-0003](../adr/0003-session-controller-reduce-diff.md).

### `unavailable`

Another input method has taken the seat. The daemon records that it has stopped, logs a warning, and stops.

## Commit Serial and Text Transactions

`zwp_input_method_v2` requires a serial on every commit call. The serial must match the most recent commit event known to the daemon. Before the first one, the serial is 0.

The daemon treats serial 0 as a **write barrier**: both text-transaction paths refuse to send protocol commits before the compositor has established the input-method connection. This prevents a race where Typio stages preedit text before the compositor can apply it, which would cause the compositor to silently drop the staged text without error.

Text payloads use one explicit transaction entry point, which stages an optional commit string and an optional preedit string with its cursor position and then sends exactly one protocol commit carrying the current serial. It is the only path for text payload commits. Non-text lifecycle and focus state uses a narrower commit path, so code reviewers can see that no preedit or commit string is being sent.

The keyboard router owns the staging boundary. It updates candidate state immediately, but stages composition-only preedit in a bounded latest-wins coalescer. The value becomes eligible after a 2 ms quiet period and cannot wait longer than 4 ms from the first update in a burst. If an engine emits real commit text, the router flushes immediately so commit order remains strict. If one key produces both commit text and a replacement preedit, both are sent in the same Wayland transaction.

This is deliberate: two fast key events can be delivered in separate reactor steps before Typio reads the compositor's next commit. Submitting every intermediate preedit as its own protocol commit can create same-serial commits where later values become stale. See [ADR-0042](../adr/0042-text-input-transaction-staging.md) and [ADR-0043](../adr/0043-bounded-preedit-coalescing.md). The host does not hold preedit for the compositor's commit event; that event is a compositor state boundary, not a text-commit acknowledgement.

## Keyboard Grab Lifecycle

The grab and its keymap handshake are **one resource** (absent → awaiting keymap → ready) that the focus controller creates and repairs on each reactor evaluation; the rules are in [Input-Method Session](input-method-session.md).

Briefly:
- Each grab incarnation has a **generation**. A key press claims the current generation, and the matching release is accepted only when the stored per-key generation still matches the active grab generation.
- When a grab is rebuilt (focus-in, resume, reconnect), the compositor may
  re-send already-in-flight keys; the generation fence discards them.
- Re-activation retains the grab but synthesizes releases for keys forwarded
  by the old field and stops their repeat chain before routing in the new
  field.
- Keys queued but not yet routed when an activation boundary arrives are
  discarded wholesale. An unrouted key belongs to the activation epoch it
  arrived in; routing it after the boundary would inject it into whatever field
  is focused next (the "wwwwww" regression, where a shortcut's letter key —
  Ctrl+W closing a browser tab — was routed into the newly focused field and
  its armed repeat kept firing).
- An armed repeat chain is stopped at every focus transition (focus-out,
  destroy-grab, focus-in, reactivate), and each expiration is re-validated
  against the key's tracking state and the current modifier mask: a key
  whose release was synthesized at a boundary, or a blocking-modifier
  (Ctrl/Alt/Super) transition since the chain was armed, ends the chain
  before any key is emitted.
- Unhandled keys are forwarded as original press/release pairs through the virtual keyboard.
- Modifier state is synced separately; modifier changes do not synthesise releases for unrelated non-modifier keys.
- Wedged-grab recovery relies on external fact sources (the resume detector and socket-hang signals) — see [Focus Controller](focus-controller.md). The former C host's emergency-exit shortcut and rejected-press-streak failsafe were not ported.

## Engine Availability and Fault Isolation

The daemon must remain responsive even when third-party engine processes are buggy, slow to initialize, or fail entirely. This section describes the patterns that prevent engine failures from bringing down the input method.

### Bounded initialization and availability queries

Engine discovery reads the worker's opening handshake before heavyweight initialization. The runtime then sends an initialization request over the private engine channel with a bounded 60-second cold-start budget, so loading dictionaries or deploying schemas can never block indefinitely.

Once initialized, the router queries the active engine with the typed availability request. That hot-path request has a 100-millisecond deadline. While the engine reports that it is still preparing, keys stay inside the input method rather than being forwarded with incomplete state; once it reports ready, normal routing resumes; a transport failure marks the engine failed and poisons the worker channel for supervised restart. No engine calls into the daemon and no raw callback crosses a thread or process boundary.

### Engine Process Isolation

Every engine runs out of process. The daemon sends lifecycle, key, mode, candidate, availability, and voice requests over Typio Engine Protocol on the private engine file descriptor. If an engine process crashes, the daemon observes a transport failure instead of taking the fault in the Wayland process. Engine code runs inside direct engine executables, not inside the daemon.

A poisoned worker is respawned **asynchronously**: the respawn and its re-initialization run on a detached thread and are installed by the next engine call. While a recovery is in flight the backend reports no engine, so keyboard keys pass through to the application instead of freezing the main loop on a multi-second spawn while the keyboard grab is held.

See the [engine contract](engine-contract.md)
for process isolation, ownership, and poisoned-channel recovery.

## Virtual Keyboard Forwarding

`zwp_virtual_keyboard_v1` is the daemon's output path for keys the engine declined to handle at all (the not-handled result). The virtual-keyboard bridge manages:

- **Keymap handoff** — when the compositor delivers a new keymap on the grab, the virtual keyboard must receive the same keymap before forwarding keys, or modifier mappings will mismatch.
- **Readiness gating** — forwarding is blocked until the keymap is confirmed, preventing modifier-sync errors during activation handshakes.
- **Fail-safe downgrade** — if virtual-keyboard health degrades (keymap deadline missed, compositor stalls), the daemon falls back to local key handling rather than forwarding broken state.

## Re-activate while focused: re-anchor, keep the grab

A subtle protocol behaviour: the compositor may send an activation while the daemon is still focused (e.g. the user clicked from one text field to another inside the same window). Treating this as a full deactivate-then-activate cycle would tear down the grab, lose the preedit round-trip, and interrupt typing.

The recorded activation fact makes this case explicit without that cost. When a commit batch carried an activation while the previous tick was already focused, the reduction emits a reactivation edge. The keyboard grab and the engine's input context are **left intact** because they belong to the input method, not the field. Transient key ownership does belong to the old field, so the daemon synthesizes releases for forwarded keys and stops the repeat timer. It then resets the Panel anchor, invalidates the submitted-frame cache, and re-presents a non-empty candidate snapshot even when its sequence number did not change. A commit with no activation in the batch keeps focus state untouched, so plain text-state updates during composition never disturb the grab. See [ADR-0018](../adr/0018-focus-transition-classification.md).

## Resume and silent grab loss

System suspend is invisible to the Wayland protocol: no deactivation before sleep, no guaranteed activation after wake; a held modifier may be stuck and the grab may be silently dead on wake. The compositor can also drop the grab with no event at all (restart, bug, race).

How much of this the focus controller handles depends on whether its observation step can *see* it — and observation reads resource *presence*, not *liveness*:

- **Suspend.** A grab dead across suspend leaves a *live proxy*; observation reports it healthy, so the focus controller alone is blind. A resume **detector** (logind's sleep signal plus a monotonic-versus-boot-time clock gap heuristic) records facts: it invalidates the grab generation and drops the compositor-visible preedit, then lets the next reactor step rebuild as needed.
- **Grab object gone.** If the grab *object* is actually absent while a grab is still wanted, observation reports it absent and the diff recreates it.

In both cases the input context is never unfocused, so the engine's in-flight composition survives, and the rebuild is the *same* grab build used on first focus.

## Indicator behaviour

The indicator (the transient Panel showing the active engine and mode label) has three show paths, each with different gate semantics:

| Path | Trigger | Gates |
|---|---|---|
| First-focus | the first activation of a session only | salience (suppress quiet modes) + acknowledged-recency (suppress if the user typed or saw the indicator within the last 3 s) |
| Reactivate | a reactivation | salience only — the user moved to a new caret in the same session, so do not suppress on recency (the new field's context can differ from the previous one's) |
| Deliberate-change | engine switch, mode change, profile toggle, or the indicator-summon shortcut | none — the user just acted, so always announce |

On deactivation, the indicator is hidden along with all other Panel UI. On reactivation, the indicator re-evaluates against the salience gate: notable modes re-show (the user has moved to a new caret whose context can differ), while quiet modes stay suppressed. The recency gate does not apply on reactivation. See [ADR-0018](../adr/0018-focus-transition-classification.md).

The indicator auto-hides after a configurable duration (1500 ms by default, clamped between 100 ms and 10 s) driven by a self-waking timer.

## Known limits: terminal multiplexers

Terminal emulators (foot, Alacritty, kitty, …) register the **entire terminal window** as a single input-method area. When a terminal multiplexer like tmux or screen splits that window into panes, sessions, or windows, those context switches happen entirely inside the terminal's own rendering — they produce **no Wayland focus events**. From the compositor's perspective, the user never left the same input field.

Consequences for indicator behaviour:

- Switching tmux panes or windows does **not** trigger an activation or deactivation, so the daemon never sees a reactivation. Any indicator that was showing stays visible until its auto-hide timer expires.
- There is no way for the input method to detect intra-terminal context switches. This is an inherent limitation of the input-method protocol model, which only knows about compositor-level focus, not application-level editing context.
- The auto-hide timer is the only mechanism that dismisses the indicator in this scenario. Lowering the duration makes the indicator vanish sooner but shortens the display window for all contexts, including those where the indicator is useful.

This limitation also affects the candidate Panel position: the input-popup surface is anchored to the terminal's cursor rectangle, not to a tmux pane boundary.

## Preedit Transaction Optimisation

When the user navigates candidates with `Up`/`Down`, only the `selected` index changes; the preedit text is identical. The daemon detects this by planning the update against the last compositor-facing preedit, including any preedit already staged for the current key drain.

If the plan says that only the Panel's own presentation changed, the Panel Scheduler refreshes only the Candidate Panel during the event-loop Panel stage. The expensive preedit round-trip to the application — a preedit set followed by a serial commit and the compositor's commit event — is skipped entirely.

If the preedit did change, the router stages the new preedit rather than committing it immediately. Composition-only bursts coalesce across adjacent reactor steps to the latest staged value, subject to the 4 ms hard limit; commit-producing keys flush immediately after the matching composition is drained so text order stays correct. This avoids both redundant round-trips and stale same-serial commits in heavyweight clients and compositors.

## See Also

- [Input-Method Session](input-method-session.md) — declared lifecycle phase, observed axes, key generation fencing, and daemon resilience (suspend/resume, compositor restart, silent grab loss)
- [Event Loop Scheduling](event-loop-scheduling.md) — event-loop scheduling, bounded Panel work, D-Bus dispatch, and poll deadlines
- [ADR-0042: Text-input transaction staging](../adr/0042-text-input-transaction-staging.md) — why preedit commits are staged/coalesced at key-drain boundaries
- [ADR-0043: Bounded preedit coalescing](../adr/0043-bounded-preedit-coalescing.md) — how adjacent cross-step keys coalesce without waiting on the compositor commit
