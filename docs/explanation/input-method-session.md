# Wayland Input-Method v2 Session

## Purpose

This document describes the lifecycle of a Wayland input-method engagement in
typio — from the moment the compositor gives the daemon focus, through
keyboard-grab setup and key routing, to teardown and recovery.

One naming hazard runs through all of it: three things span that engagement
with **different lifetimes**, and two of them are called "session." Telling
them apart is what separates a precise focus/grab/preedit bug report from a
vague one, so the next section pins them down before the lifecycle detail.

See also: the upstream protocol spec
([`input-method-unstable-v2.xml`](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/unstable/input-method/input-method-unstable-v2.xml));
the control model that manages the grab resource
([Focus Controller](focus-controller.md)); event-loop scheduling
([Event Loop Scheduling](event-loop-scheduling.md)).

## Three Lifetimes to Keep Separate

Three things span an input-method engagement, each with a **different**
lifetime. Two carry the word "session"; the third is the focus controller's
resource (it was the "session controller" until that name was dropped to stop
exactly this collision). Confusing the three is the most common source of vague
focus/grab bug reports.

| Concept | What it is | Lifetime | Owner |
|---------|-----------|----------|-------|
| **Protocol session** | One activation-to-deactivation cycle on the input-method protocol object | compositor activation → deactivation | compositor |
| **Session state** | The frontend's editing facts plus the input context the keyboard router owns | first activation → frontend teardown | daemon |
| **Grab / focus lifecycle** | The keyboard-grab plus virtual-keyboard-keymap resource | "a grab is wanted" → "none wanted" | [focus controller](focus-controller.md) |

The first two are detailed below; the third is the [focus controller](focus-controller.md)'s
domain and is only summarized here.

### Protocol Session

The compositor owns this concept. It begins when the compositor sends an
activation event and ends when it sends a deactivation. Events inside one
session travel through the protocol object the daemon bound at startup.

A protocol session is **not** always followed by a full teardown:

- **Normal deactivate** ends the protocol session but the daemon may retain
  the keyboard grab in a **soft pause** so the next activation skips the
  expensive rebuild (keymap compile, Wayland grab create, awaiting-keymap
  window).

- **Reactivation** (activation while already active, with no intervening
  deactivation) extends the same protocol session. The compositor moved focus
  to a new text field inside the same window. The daemon keeps the grab and
  the engine's in-flight composition, but releases old-field forwarded keys,
  stops their repeat chain, and re-presents candidates at a refreshed Panel
  anchor.

The commit event is the compositor's double-buffer commit point. Events in a
batch (activation, deactivation, surrounding text, …) are provisional until
that commit is delivered, which applies them atomically. The daemon records
them as facts and classifies the batch at commit time; see
[ADR-0018](../adr/0018-focus-transition-classification.md).

ADR-0018 describes that classification as a pure helper taking three inputs —
whether the session was active, whether it is active now, and whether the batch
carried an activation — and reducing them to activation, reactivation,
deactivation, or no-op. The tree does not carry that helper as a named
function: the classification is realised as the recorded batch facts the
derived-state step consumes, which the ADR accepts as a pragmatic step away
from the pure-derived ideal.

### Session State

The platform frontend stores compositor editing facts, while the keyboard
router owns the corresponding runtime input context. No pointer or
callback-user-data relationship connects them.

Lifetime rules:

- **Created** during daemon/frontend initialization.
- **Updated** by compositor events and committed at each double-buffer commit
  boundary.
- **Reused** across deactivate/reactivate focus churn; the input context
  remains owned by the router.
- **Destroyed** during ordered daemon shutdown after focus-out and before the
  runtime instance.

The session state **survives** a deactivation. It is not tied one-to-one to
the protocol session. This matters for two reasons:

1. The engine's in-flight composition survives a soft pause. A quick
   focus-out/focus-in (clicking from one field to another) does not reset the
   engine.
2. Surrounding text and content-type facts are cached across activations so
   the engine sees context immediately on re-focus.

### Grab / Focus Lifecycle

This is the availability of the unified keyboard-grab plus virtual-keyboard
keymap resource — **not** a second protocol object and **not** another session
store. It can outlive a protocol session (soft pause) and be rebuilt across
protocol sessions (resume, reconnect).

It is owned by the **focus controller**, whose derive/diff/apply pipeline
converges it onto the desired state derived from input facts. Its readiness
states (absent, awaiting keymap, ready), the state table, and the
rules that govern them are the mechanism's single source of truth — see
[Focus Controller § Actual State](focus-controller.md#actual-state).

## Build-up Chain

The most failure-sensitive chain is the **build-up order** that lifecycle
transitions must preserve when bringing a focused session to a state where
keys reach the engine and unhandled keys reach the app:

1. Input-method activation (a focus fact arrives).
2. Keyboard-grab creation.
3. Compositor keymap delivery on the grab.
4. Keymap forwarding into the virtual-keyboard object.
5. Virtual-keyboard transition to ready.
6. Only then: unhandled-key forwarding to the focused application.

If that chain is incomplete or reordered, the frontend must not behave as if
virtual-keyboard forwarding is healthy.

If a keyboard or focus bug appears "sometimes", treat it as a build-up-chain
problem first, not as a one-off key handling bug.

## Truth Sources

Each input fact has exactly one source. Facts are recorded, never interpreted
at arrival:

| Fact | Source |
|------|--------|
| Activation, deactivation, and the commit serial | focus plus the compositor double-buffer commit point |
| Key press and release | physical key truth (carries the current grab epoch) |
| Modifiers | modifier-mask truth |
| Repeat info and repeat timer | repeat truth |
| Surrounding text and content type | client editing context |
| Suspend gap, connection up/down | environment truth |
| Virtual-keyboard output | **side effect only, never a source of internal truth** |

Do not derive lifecycle truth from forwarded virtual-keyboard output.
Additionally:

- a live keyboard grab is not proof that the virtual keyboard is ready
- a previously healthy virtual keyboard is not proof that the current grab
  has a current keymap

The observed snapshot is a **view of reality, never a stored second source of
truth**, so it cannot drift from the resources it describes.

## Ownership

Each decision in this lifecycle has exactly one owner:

- **Pure lifecycle decisions** — what the desired state is, and which effects
  follow from the difference against the observed state — plus the data
  structures that describe them.
- **The effectful side** — observing the live resources, and executing the
  effects.
- **Press ownership** — keyboard epochs and symmetric press/release
  tracking. It is mutable bookkeeping and **never** the routing decision.
- **The routing decision** — given a key, the modifier state, and the current
  state, whether to consume or forward, and why. It is pure.
- **Key-event interpretation** — the XKB decode into an owned key event, while
  the session is focused.
- **Virtual-keyboard health** — keymap and modifier handoff, readiness gating,
  and the fail-safe downgrade.
- **Poll scheduling** — bounded auxiliary-descriptor dispatch and
  deadline-aware wakeups.
- **Config watching** — watch events, debounce timing, watch rearming, and the
  runtime reload boundary.
- **Voice** — recording and inference state, plus deferred voice reload
  application.
- **The logical modifier view** — the XKB state.
- **Engine implementations** — only engine and composition behavior.

The status surface exports this state but does not own it. The runtime-state
projection is a read-only view of observation, not an independent tracker.

## Three Lifetimes Across Two Protocol Sessions

The sequence below shows how the three lifetimes interleave across a pause
between two protocol sessions.

| Stage | Session state | Grab resource |
|-------|---------------|---------------|
| Protocol session 1: activation plus commit | active | created, awaiting keymap |
| Protocol session 1: typing | context retained | ready, keys reach the engine |
| Protocol session 1: deactivation | paused | soft-paused, stays ready |
| Pause, with no protocol session | context still alive | retained, ready |
| Protocol session 2: activation | active | reused, still ready |
| Protocol session 2: typing | context retained | ready, keys reach the engine |

Key observations from the sequence:

- The frontend session state and the router-owned input context live across both protocol sessions.
- The grab is created once, stays ready through the soft pause, and is
  reused on the second activation.
- The focus controller's wanted grab transitions from "grab" to "soft pause"
  to "grab". No grab destruction effect fires during the soft pause.

## Grab Readiness

The keyboard grab and its virtual-keyboard keymap handshake are **one
resource** with a single readiness state (absent, awaiting keymap,
ready) — no separate phase plus virtual-keyboard state machine plus "non-routable grab"
rescue branch. Keys route to the engine only when the resource is ready.

The readiness states, their state table, and the epoch rules that govern
them (new epoch on rebuild, an old ready state never survives, and the
modifier-versus-key gating during the awaiting-keymap state) are owned by the
focus controller — see
[Focus Controller § Actual State](focus-controller.md#actual-state).

## Engine Availability

Grab readiness is necessary but not sufficient. The active keyboard engine also
has an availability axis (uninitialized, preparing, ready, failed), queried
over the wire for the control surface and voice gating. The key path does not
poll it per key; instead it relies on the poisoned-channel rule:

- a healthy engine answers a key within a 100 ms deadline;
- a transport error poisons the worker, which is discarded and respawned
  **asynchronously** — while the respawn and re-init is in flight the backend has
  no engine, so keys come back unhandled and are forwarded to the
  application instead of freezing the main loop on a multi-second spawn;
- repeated respawn failures back off exponentially (1→2→…→64 s), so an
  engine that crashes on startup is retried roughly once a minute rather
  than once per keystroke; the first retry after any successful request is
  immediate.

A key routed during that recovery window never starts a repeat chain against
the engine and never blocks the grab.

## Keyboard Epochs and Press Ownership

Every focus boundary, keymap replacement, and grab teardown starts a new
keyboard epoch. Queued presses from an older epoch cannot enter the new field.
Releases still reach the virtual-keyboard ledger to pair presses already sent
to an application. A boundary releases any remaining forwarded keys.

Active input and inactive pass-through use the same ledger and ordered stream.
Modifier changes cannot overtake queued letters. A physical release after a
synthetic release is harmless because the ledger no longer owns that press.
A fresh press starts new ownership without inheriting a release marker.

Applications repeat forwarded keys themselves. The host repeats only keys
consumed by the engine or candidate selection, while they remain physically
held and owned by the current session. Focus boundaries and blocking-modifier
changes end that repeat chain.

## Teardown

Every transition that ends a grab — focus loss, suspend, reconnect, fail-safe,
or observed-axis repair — runs the **same** teardown path:

- forwarded keys are released to the virtual keyboard
- virtual-keyboard modifiers are reset to zero (exception below)
- key repeat is cancelled
- the grab object is destroyed and a new epoch is begun
- per-key tracking is cleared
- any stale assumption that the virtual keyboard is ready is discarded; the
  next epoch must re-earn readiness

The one exception is a focus handoff (the derived "activating from focused"
case): the last compositor-reported modifier mask may be carried to the virtual
keyboard so the newly focused client can still observe a held shortcut modifier.
Carried modifier state must be cleared before the next grab is built. A
suspend/reconnect teardown carries nothing — a modifier held across the boundary
produced no key-up and is dropped unconditionally.

## Recovery

Recovery shares the normal path **only for divergences the observed axes can
see**. Observation reads resource *presence*, not external liveness, so the
focus controller's diff is a backstop for internal state drift, not a detector
of silent compositor-side grab death:

- **Internal divergence** — the grab object is missing while a grab is still
  wanted. Observation reports it absent, so the diff recreates the grab on
  the next reactor step.
- **Suspend/resume** — a grab dead across suspend can leave a live proxy, which
  observation cannot distinguish from a healthy one. A resume detector records
  the gap fact and invalidates the grab generation; the next reactor step
  rebuilds. The input context is never told that focus left, so the engine's
  in-flight composition survives.
- **Compositor reconnect** — connection death surfaces as a socket hangup; the
  lost connection forces the wanted grab to "none" and a full teardown, and the
  fresh activation on reconnect drives the rebuild. Engine and session state,
  auxiliary handlers, the config watch, and the resume detector are preserved.

A grab the compositor orphans with *no* protocol event, suspend, or disconnect
is invisible to observation and is **not** auto-recovered. The focus controller
can only act on facts it can see.

### Suspend Without Deactivate

The compositor may not send a deactivation before the system sleeps. The
resume detector records a suspend gap, which forces the wanted grab to "none".
The grab resource is torn down and rebuilt
proactively, even though no protocol session boundary was crossed.

### Compositor Restart Without Events

The compositor crashes and restarts. The Wayland socket stays open (no hangup)
but the new compositor does not restore the old grab. The
protocol session is still "active" from the daemon's point of view, but the
grab resource is silently dead. This is the **dead-but-present** blind spot:
observation sees a live proxy, so the diff produces no
effects. Recovery requires an external fact source (resume detector or future
liveness probe); see [Focus Controller](focus-controller.md) § Blind
Spot.

### Engine Switch Mid-Session

Switching the active language retargets the keyboard engine
([ADR-0031](../adr/0031-language-first-switching-surface.md)). This is **not**
a protocol event, so it does not start a new protocol session. The old engine's
preedit is cleared, the Panel is hidden, and the new engine receives focus
for the same input context. The grab resource stays ready throughout.

## Shortcut Policy

Application shortcuts are decided in the Wayland frontend, as a pure routing
decision:

- routing yields two independent dimensions: whether to consume or forward the
  key, and why; the per-key tracker records lifecycle history (forwarded,
  app-shortcut, or consumed) for symmetric release, and is **not** the routing
  model
- non-modifier keys with Ctrl, Alt, or Super bypass the engine; the matching
  release must also bypass it
- Typio-reserved shortcuts (emergency exit, voice push-to-talk) are consumed
  internally and never treated as virtual-keyboard forwarding
- emergency exit is the highest-priority reserved decision on key press: dump
  recent logs, release the grab, stop the frontend — it forwards no key
- engines do not each implement shortcut bypass; modifier-only chords such as
  the language switch stay transparent to the app and the compositor
- on language-switch completion, the arbiter clears the old engine's
  composition, the compositor-facing preedit, and the candidate panel before
  activating the new engine

## Invariants

- lifecycle transitions must go through the focus controller's single update
  entry point and its desired/actual state reconciliation
- observed lifecycle axes must be used to detect declared-phase drift, not as
  a second mutable phase model
- no key press or release is processed unless the grab resource is ready
- a poisoned engine worker is never reused; during its asynchronous respawn
  window keys pass through to the application instead of blocking the main loop
- an engine inactive for 15 minutes has its worker process stopped by the
  idle reaper; re-activation transparently re-spawns it (deactivation is
  otherwise protocol-level only, so without reaping every engine ever
  activated would stay resident for the daemon's lifetime)
- modifier-mask updates may be processed while the grab awaits its keymap, to
  resynchronize held modifiers
- no virtual-keyboard forwarding happens unless the virtual keyboard is
  explicitly ready
- an obsolete queued press is dropped; its release may only finish an
  existing virtual-keyboard press
- no per-key tracking state survives a teardown
- application shortcut press and release stay symmetric
- a rebuilt grab never inherits prior-epoch keymap health
- a grab **retained** across a soft pause (a focus-out that keeps the grab to
  skip rebuild) must still shed host-side input-arbitration state — the
  physical modifier view, the "a blocking modifier was seen" mark, and the
  shortcut arbiter — so a modifier held at defocus, whose release the routing
  guard drops, cannot phantom-persist into the next activation and corrupt
  chord detection
- fail-safe paths prefer releasing the grab over running partially broken
- an engine-switch failure must not silently clear the previously active engine
  in that category
- engine switch clears composition, preedit, and candidate panel before the
  new engine activates — no stale underlined text survives an engine boundary

## Test Expectations

Session lifecycle regressions should be covered by:

- focus-controller tests: the reduce and diff decisions, and focus-edge
  detection
- keyboard ownership tests: epoch fencing and paired press/release across
  teardown and active/inactive transitions
- routing tests: the pure decision over key, modifiers, and state, including
  reserved shortcuts
- repeat tests: the armed-chain guard — states that must not repeat, suppressing
  modifiers, and post-arm blocking-modifier transitions
- platform tests: keyboard event order across every dispatch partition of a
  shortcut handoff, repeated text-state batches, and modifier-mask mapping

Every guard deleted from the old model (startup suppression, boundary carry,
divergence repair) must first be re-expressed as a failing focus-controller,
state-machine-property, or helper-policy test before its imperative code is
removed.

## See Also

- [Focus Controller](focus-controller.md) — the control model that
  manages grab resources
- [Event Loop Scheduling](event-loop-scheduling.md) — frame bounds,
  D-Bus dispatch, config reload, and poll deadlines
- [Wayland Input Method Protocol](wayland-input-method.md) — how the daemon
  implements the protocol handlers
- [Input Session Blueprint](../architecture/input-session.md) — the current
  session, grab, and transaction implementation
- [ADR-0018: Focus Transition Classification](../adr/0018-focus-transition-classification.md)
  — how the commit boundary classifies a batch into activation, reactivation, or no-op
- [ADR-0003: Session Controller — Derived State, Idempotent Diff](../adr/0003-session-controller-reduce-diff.md)
  — the architectural decision that introduced the reduce/diff/apply model
