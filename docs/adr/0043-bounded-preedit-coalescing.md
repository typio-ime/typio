# ADR-0043: Bounded Preedit Coalescing Across Reactor Steps

- **Status**: Accepted
- **Date**: 2026-07-11
- **Deciders**: Project maintainers

## Context

[ADR-0042](0042-text-input-transaction-staging.md) coalesces pure preedit
updates while the keyboard router drains one pending-key batch. Two physically
adjacent keys can still arrive in separate reactor steps because the compositor,
kernel socket, and `poll(2)` determine event-delivery boundaries. The first step
can therefore submit `set_preedit_string("j") + commit(S)` before the second step
produces `set_preedit_string("jr") + commit(S)`.

The engine and host candidate projection already contain `"jr"`, but the later
same-serial transaction can become visually stale as the compositor advances
its input-method state. This produces a missing inline letter while candidate
selection and commit behavior remain logically correct.

A previous follow-up held subsequent pure preedit until compositor `done`.
`done` is a state boundary, not a text-commit acknowledgment, so that gate could
wait indefinitely and made ordinary composition lag.

The event loop also embedded keyboard ordering, Panel presentation, raw
`pollfd` indices, status timers, voice, and configuration handling in one
898-line function. That concentration made the accidental pending-key boundary
look like a protocol boundary.

## Decision

Pure preedit uses a bounded latest-wins coalescer:

- an update becomes eligible after 2 ms without another preedit update;
- the first update in a burst has a fixed 4 ms hard deadline;
- later updates replace the payload and renew only the quiet deadline;
- candidate state and Panel dirtiness update immediately;
- real `commit_string` output bypasses the deadline and flushes immediately;
- focus loss, reset, soft pause, and engine reset clear staged preedit;
- the coalescer's earliest deadline participates in the main `poll(2)` timeout,
  so idle operation remains wakeup-free.

The Wayland loop is organized as a reactor coordinator. Named poll sources and
deadline reduction live in `app/reactor.rs`; ordered keyboard/text and repeat
effects live in `app/input_driver.rs`; candidate Panel convergence lives in
`app/panel_driver.rs`. `event_loop.rs` retains I/O preparation and the visible
phase order. One-shot status timers own their timerfd and armed state together;
because the fd wakes `poll(2)` directly, no mirrored user-space deadline is
kept.

## Alternatives considered

- **Wait for compositor `done`**: rejected because `done` does not acknowledge a
  text transaction and can leave preedit waiting indefinitely.
- **Allow only one text commit per serial**: rejected for the same liveness
  reason and because real commit text must never wait.
- **Increment or predict the serial**: rejected because the protocol requires
  the count of `done` events actually received.
- **Flush every event-loop iteration**: rejected because an implementation
  scheduling boundary is not a reliable input-transaction boundary.
- **Adopt an async runtime or dynamic event bus**: rejected because the runtime
  has a small fixed fd set, Wayland objects are thread-affine, and explicit
  input ordering is a correctness property.

## Consequences

- Positive: physically adjacent keys coalesce even when they cross one
  pending-key drain, preventing the visually missing second preedit letter.
- Positive: the worst-case added pure-preedit latency is 4 ms, below one display
  frame, while commit text remains immediate.
- Positive: every time-based wake is an explicit deadline; no periodic idle tick
  is introduced.
- Positive: reactor source identity and subsystem boundaries are named and
  independently testable.
- Trade-off: pure preedit display intentionally trails engine state by up to
  4 ms.
- Trade-off: the implementation has more small modules, but each module owns a
  cohesive scheduling responsibility.
