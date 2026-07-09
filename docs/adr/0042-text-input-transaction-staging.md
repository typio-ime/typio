# ADR-0042: Stage text-input transactions at key-batch boundaries

- **Status**: Accepted
- **Date**: 2026-07-09
- **Deciders**: Project maintainers

## Context

`zwp_input_method_v2` applies text updates through a double-buffered transaction:
`commit_string`, `set_preedit_string`, and related requests stage pending state;
`commit(serial)` applies that state.  The serial is tied to the compositor's
`done` stream, so a daemon can legitimately process multiple key events before it
has had a chance to read the next `done`.

The old keyboard path flushed Wayland text state directly from each engine
composition callback:

```text
key -> engine composition -> set_preedit_string -> commit(serial)
```

When two key events were delivered in one pending-key drain, this could produce
multiple `commit(serial)` requests with the same serial:

```text
key 1: set_preedit_string("n")  + commit(S)
key 2: set_preedit_string("ni") + commit(S)
```

Depending on compositor timing, the first commit can advance the compositor-side
serial before the second request is applied, making the second request stale.  In
practice this showed up with Rime as fast typing where the engine had produced
the second preedit/candidate state, but the inline preedit and candidate UI did
not reliably reflect it.

The problem is not an engine contract problem: engines should still emit a full
`TypioComposition` after each handled key.  The issue is the host's mapping from
engine output to Wayland text-input protocol commits.

## Decision

The host treats Wayland text updates as explicit transactions and stages them in
`KeyboardRouter` before flushing them through one platform entry point:

```rust
InputMethodState::text_transaction_and_flush(
    commit_text: Option<&str>,
    preedit: Option<(&str, u32)>,
)
```

Rules:

1. Engine composition still updates host memory immediately: candidate content,
   selected index, host-managed-selection flags, and panel dirty state are
   current as soon as `drain_composition` runs.
2. Composition-only preedit updates are staged in
   `KeyboardRouter::pending_preedit_flush` and coalesced to the latest value for
   the current pending-key drain.
3. Engine commit text is staged in `pending_commit_flush` and flushed before the
   next key is routed, preserving text order.
4. When a key produces both commit text and a replacement preedit, the router
   flushes them as one Wayland transaction: `commit_string`,
   `set_preedit_string`, then one `commit(serial)`.
5. Lifecycle/focus commits that carry no text payload use
   `InputMethodState::commit_protocol_state`; text paths must not call raw
   `commit(serial)` directly.

The old direct helpers (`commit_string_and_flush`, `set_preedit_and_flush`, and
`clear_preedit_and_flush`) are removed so new code cannot accidentally bypass the
transaction staging model.

## Consequences

- **Positive**: fast composition-only bursts such as Rime `n` + `i` submit only
  the final preedit (`"ni"`) for that drain boundary, avoiding stale same-serial
  intermediate commits.
- **Positive**: commit+remaining-preedit cases are represented as the protocol's
  natural atomic transaction instead of two separate commits.
- **Positive**: candidate panel state remains low-latency because it is updated
  in host memory immediately; only compositor-facing inline preedit commits are
  coalesced.
- **Trade-off**: `KeyboardRouter` now owns a small text-transaction staging
  state.  That complexity is intentional: it is the boundary where engine event
  ordering and Wayland serial semantics meet.
- **Invariant**: outside `InputMethodState`, all text payload commits go through
  `text_transaction_and_flush`; raw `commit_protocol_state` is reserved for
  non-text lifecycle state.

## Follow-up: preedit-only serial gate (cross-tick)

Batch-boundary coalescing alone is not enough. Two keys can land in *different*
event-loop ticks before the compositor replies with `done`, so the host would
still issue:

```text
tick 1: set_preedit("n")  + commit(S)
tick 2: set_preedit("ni") + commit(S)   // done for S not yet received
```

`InputMethodState` owns a `TextSerialGate`:

- **Preedit-only**: first `commit(S)` proceeds; further pure preedit for the
  same serial is deferred and flushed when `done` advances the serial (or when
  a later `commit_string` forces a send).
- **`commit_string`**: always sent immediately. Compositor `done` is not an
  acknowledgment of the client's text `commit` — waiting for it stalls Space
  上屏 until an unrelated `done` or another key arrives.

Engine/candidate state is still updated immediately.

## Related

- [ADR-0002: Adopt `zwp_input_method_v2` as the Host Protocol](0002-wayland-input-method-v2.md)
- [Event Loop Scheduling](../explanation/event-loop-scheduling.md)
- [Wayland Input Method Protocol](../explanation/wayland-input-method.md)
