# ADR-0052: Ordered Keyboard Events and Press Ownership

- **Status**: Accepted
- **Date**: 2026-09-15
- **Scope**: Input session, platform transport, keyboard router
- **Amends**: [ADR-0003](0003-session-controller-reduce-diff.md) by extending the single epoch fence to focus boundaries and keymap replacement
- **Supersedes**: [ADR-0018](0018-focus-transition-classification.md), decision 1's per-text-batch focus-fact lifetime; preserves grab retention and re-anchoring

## Context

A shortcut can move focus before its physical release reaches the router.
The host queued keys but immediately forwarded modifiers, discarded the key
queue at focus boundaries, and tracked synthetic releases independently in the
router and platform. Multiple text-state `done` events in one dispatch could
overwrite the focus fact before the controller consumed it. Together these
paths could lose the release of a forwarded shortcut letter and leave the
application repeating it in the next field.

This decision crosses the daemon/platform boundary and establishes binding
ownership and event-order invariants. It retains the derived resource
controller; the defect is in the facts and keyboard transport feeding it.

## Decision

1. Keep the final active level separate from an unconsumed focus-boundary bit.
   Only controller observation consumes that bit. Text-state `done` advances
   the serial and editing snapshot without changing focus-edge lifetime.
2. Queue keys, modifier samples, and focus boundaries in one ordered stream.
   Active and inactive input use the same stream. Each key captures its epoch
   and host modifier sample when received.
3. Advance one keyboard epoch at activation, deactivation, keymap replacement,
   and grab teardown. Obsolete presses cannot enter a new field. Releases may
   still pair application-owned presses; they cannot enter a new engine
   gesture. Ignore events from destroyed grab proxies.
4. The platform owns the sole virtual-keyboard press ledger. A boundary
   releases all remaining presses. Duplicate presses and orphan releases do
   not produce protocol requests. No synthetic-release marker or second
   forwarding ledger survives in the router.
5. The router records current gesture ownership for engine release routing.
   Physical hold state is separately observed by the platform. Both are
   required for consumed-key repeat. An unowned modifier release clears its
   physical baseline without completing a shortcut.
6. Applications repeat forwarded presses normally. The host timer repeats only
   consumed keys that XKB marks repeatable. Focus boundaries and any blocking
   modifier transition cancel the chain. Releasing an unrelated key does not.
7. Keymap replacement and grab teardown release forwarded keys before changing
   the transport resource and invalidate queued events from its old epoch.

## Invariants & Behavioral Boundaries

- Ordinary text-state updates cannot erase or replay a focus boundary.
- Modifier output and key output preserve their observed relative order.
- Every virtual-keyboard release has an owned press; every boundary ends all
  application-owned presses from the preceding field.
- A queued press is routable only in its captured keyboard epoch.
- An inactive pass-through press has the same release guarantees as active input.
- No engine release or host repeat may complete a gesture from a prior epoch.
- Keymap readiness requires successful compilation and virtual-keyboard handoff.

## Rejected Alternatives & Negative Knowledge

- **OR the existing per-`done` flags.** This prevents one overwrite but retains
  two focus-fact lifetimes and activation-first precedence that misclassifies
  a batch ending in deactivation. Observe a final level and one boundary bit.
- **Discard all queued keys.** Losing a release for an already-forwarded press
  can leave application-native repeat running even after the host timer stops.
- **Only stop the host timer.** Application repeat has a separate owner and
  needs a virtual-keyboard release.
- **Snapshot modifiers without ordering their output.** Correct engine input
  would still leave applications receiving modifier updates ahead of letters.
- **Keep the inactive fast path.** It bypasses ownership tracking and makes
  correctness depend on which side of a focus boundary the release arrived.
- **Retain per-key generation arrays and synthetic-release sets.** They duplicate
  ownership and introduce stale markers. Queue epochs and the transport ledger
  cover the required boundaries directly.

## Validation

Platform tests exercise 2,048 dispatch partitions of a Ctrl+W handoff, including
release-before-boundary and release-after-boundary sequences. They check output
pairing and modifier order using the production transport preparation path.
Additional tests cover repeated `done`, both focus-event orders, inactive
presses, grab teardown, and router repeat cancellation. Real browser acceptance
remains a separate journey in [Acceptance](../dev/acceptance.md).

## Consequences

Resource convergence stays in `reduce`/`diff`/`apply`. Keyboard transport effects
follow the ordered stream after convergence. Composition preservation on a
coalesced field handoff remains unchanged. A keymap replacement is a hard
keyboard boundary even if the input context itself remains focused.
