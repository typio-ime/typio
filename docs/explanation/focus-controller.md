# Focus Controller

## Purpose

The focus controller is the per-tick control loop that manages typio's
Wayland input-method **focus and keyboard-grab lifecycle**: grab create/destroy,
focus-in and focus-out, keymap epoch scrubbing, and discarding an abandoned
composition on defocus.

It holds **no stored lifecycle phase**. The only persisted things are raw input
facts and live resource handles. Every event-loop tick derives what the
resources *should* be from what has happened, then converges them with a
minimal, idempotent diff — so recovery is the normal path run against changed
facts, not a separate branch.

## Design Declaration

1. **No stored lifecycle phase.** The only persisted things are raw input facts and live resource handles. Every tick derives what the resources *should* be from what has happened.
2. **All effects are idempotent and applied as a diff.** Applying the same desired state twice is a no-op. Recovery is not a separate code path; it is the normal path run against changed facts.
3. **Decision logic is pure and testable.** The reduce step (facts to desired state) and the diff step (desired against actual to effects) are pure functions. They do not touch the frontend, Wayland, or I/O.
4. **Observe reads presence, not liveness.** The diff converges the host's own state. It cannot detect a resource that is dead but still present as a client-side proxy. That is a structural limitation of the observation layer, not a bug to be patched inside the diff.

## Data Flow

Every event-loop iteration runs one step, in this order:

1. **Record** the facts the tick is acting on: input-method events, a suspend gap, connection state.
2. **Reduce** the facts, together with the previous tick's desired state, into the resource configuration the host wants.
3. **Observe** the live resources for a snapshot of what the host actually has.
4. **Diff** the desired state against the actual state to obtain a minimal, idempotent effect set.
5. **Apply** the effects — create or destroy the grab, deliver focus transitions, and so on.

### Facts

A fact is a recorded event with exactly one source. Facts are never interpreted at arrival.

| Fact | Source |
|------|--------|
| Activation observed | the input-method activation event handler |
| Deactivation observed | the input-method deactivation event handler |
| The commit batch carried an activation | classification of the double-buffer commit batch (distinguishes reactivation from a plain text-state update) |
| Protocol serial of the commit point | the compositor's double-buffer commit event |
| Connection alive | absence of a hangup on the Wayland socket |
| Suspend gap detected | the system resume detector (logind sleep notification or boottime-gap heuristic) |

### Desired State

The reduce step derives the wanted resource configuration. It uses the previous
tick's desired state for edge detection on focus-in and focus-out.

| Wanted grab | Meaning |
|-------------|---------|
| None — hard teardown | The connection is gone, or the system resumed from sleep |
| Soft pause — retain the grab for reuse | A normal deactivation |
| Grab — the grab must exist and be ready | Focus is established |

The final focus state determines the resource target. A dead connection, a
suspend gap, or the absence of a keyboard engine requires teardown. Otherwise,
an active field requires a grab; a deactivated session keeps its grab in a
soft pause. When several focus events arrive together, their final state wins.

**Soft pause.** Normal deactivation releases forwarded keys and stops repeat,
while retaining the grab and keymap for reuse. Gesture ownership resets; the
next field starts a new gesture with the current modifier sample.

**Focus edge detection:**

- focus-in when the wanted grab becomes "grab" and the previous tick's was not
- focus-out when the wanted grab stops being "grab" and the previous tick's was

The edge detection prevents repeated focus-in and focus-out calls while the
state is stable across multiple ticks.

**Reactivate.** A focus boundary observed while the session remains active
sets the reactivate edge. The boundary remains recorded until the controller
observes it; ordinary text-state updates cannot erase it. A boundary that has
already been observed cannot fire again when a text-state batch completes. The grab and the
in-flight composition are preserved. Transient key ownership is not: releases
are synthesized for keys forwarded by the old field, and its repeat chain is
stopped. The Panel anchor is refreshed, and any unchanged candidate snapshot
is re-presented because the compositor may have unmapped the popup during the
handoff.

### Actual State

The observe step returns a read-only snapshot of the live frontend fields. It
is **not** a second source of truth.

| State | Trigger | Next state |
|-------|---------|------------|
| Absent — no grab object | the grab is created | Awaiting keymap |
| Awaiting keymap — the grab exists, its keymap handoff is pending | the compositor's keymap arrives and is mirrored to the virtual keyboard | Ready |
| Ready — the grab exists and its keymap is synced for the current epoch | the grab is destroyed | Absent |

The grab resource state merges the grab object's presence with
virtual-keyboard keymap readiness, tracked by the platform input-method layer
as "a keymap was received in this epoch". This is one resource with one state,
not a phase plus a separate virtual-keyboard state machine.

**Readiness rules** (the single source of truth for grab readiness):

- creating or rebuilding the grab starts a new epoch and forces the
  awaiting-keymap state
- an old ready state must never survive into a new grab epoch
- ready requires a compositor keymap observed in the current epoch
- a timeout while awaiting a keymap is a fail-safe condition — prefer releasing
  the grab over forwarding through a partially broken path; the stall is
  diagnosed by the host's record of pending Wayland responses, which feeds the
  poll deadline
- modifier-mask updates may apply while awaiting a keymap (a grab built while a
  field stays focused), so held Ctrl/Alt/Super survive grab creation before the
  first new key press; key presses may not

### Effects

The diff produces a minimal, idempotent effect set:

- When a hard teardown is wanted and any grab resource is present: destroy the
  grab, begin a new keymap epoch, abandon the in-flight composition and its
  candidate UI, clear the compositor-facing preedit, and commit the pending text
  transaction.
- When a grab is wanted — freshly, or after a soft pause — and no grab resource
  exists: create the grab and begin a new keymap epoch.
- On a focus-in edge: tell the client that focus entered.
- On a focus-out edge: tell the client that focus left, abandon the in-flight
  composition and candidate UI, clear the preedit, and commit the pending text
  transaction.
- On a reactivate edge: fence the old field's key and repeat state, then
  re-anchor and re-present the Panel.

Abandoning the composition drops the engine's in-flight composition and
candidate UI when a field loses focus, so a half-typed attempt cannot leak into
the next field or auto-commit into the one being left. Both defocus paths —
soft pause and hard teardown — drive it.

### Apply

The effects execute in a fixed order:

1. Abandon the in-flight composition and its candidate UI while focus is still
   held.
2. Tell the client that focus left.
3. Destroy the grab, running the same teardown path as an emergency keyboard
   reset.
4. Clear the compositor-facing preedit.
5. Commit the pending text transaction.
6. Begin a fresh keymap epoch.
7. Create the grab.
8. Tell the client that focus entered.
9. Reactivate — fence the old field's key and repeat state, then re-anchor and
   re-present the Panel at the new caret.

The order matters: the abandoned composition is discarded before focus leaves,
teardown happens before the grab is recreated, and focus enters after the new
grab is ready.

## Per-Tick Workflow

The pipeline runs once per event-loop iteration, after Wayland events have been
dispatched and before auxiliary I/O (D-Bus, config reload, voice). This ordering
ensures that input facts are fresh before any non-input work can delay the diff.

## Blind Spot: Dead-but-Present Resources

The diff is the backstop for the host's own state, not a detector of external
silent loss.

### What the Diff Cannot See

A resource whose client-side proxy still exists but whose compositor-side state
has been silently discarded. The canonical example is a keyboard-grab protocol
object that survives a compositor restart: the client sees a non-null handle, so
observation reports a ready grab, but the compositor no longer routes key events
through it.

Another example is a stuck Wayland connection: the socket is open, so the
connection reads as alive, but the compositor has stopped processing events.
Observation reports nothing wrong, so the desired state keeps the grab, and the
diff produces no effects. The user cannot type, and the controller sees no
reason to act.

### Why This Is Accepted

Detecting silent death requires a liveness probe (heartbeat, roundtrip timeout,
or harmless request echo). Any such probe has trade-offs:

- **False positives** during legitimate idle periods (user away from keyboard) cause unnecessary grab teardown and rebuild, producing visible input stalls.
- **Protocol interference**: a periodic harmless request may still mutate client state or increase power use.
- **Threshold problem**: the timeout must be longer than any normal stall (a heavy compositor frame) but short enough that users do not notice the wedge. No single threshold satisfies both.

The accepted mitigation is external fact sources, not the diff:

- **Resume detector**: system suspend/resume is a strong signal that compositor state may be stale. It records a suspend gap, forcing a full scrub and rebuild.
- **Connection hangup**: socket death is unambiguous. It forces teardown.
- **Emergency exit shortcut**: a user-facing escape hatch (release grab plus stop the daemon) for the rare case where silent death occurs without suspend or disconnect.

A future liveness probe may be added as a new fact source feeding into the reduce
step, but it does not belong inside observation or the diff.

## See Also

- [ADR-0003: Session Controller — Derived State, Idempotent Diff](../adr/0003-session-controller-reduce-diff.md) — the architectural decision that introduced this model
- [Input Session Blueprint](../architecture/input-session.md) — the current fact, reduce, and effect implementation
- [Input-Method Session](input-method-session.md) — the three layers of "session", build-up chain, and lifecycle rules
- [Event Loop Scheduling](event-loop-scheduling.md) — frame bounds, D-Bus dispatch, and poll deadlines
- [Wayland Input Method Protocol](wayland-input-method.md) — the protocol whose events feed the reduce step
