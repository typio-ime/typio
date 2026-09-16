# ADR-0016: Out-of-process active-mode reflection

- **Status**: Accepted
- **Date**: 2026-06-06
- **Deciders**: Typio maintainers

## Context

[ADR-0011](0011-engine-mode-as-first-class-concept.md) made the keyboard engine
*mode* (中/英, かな/_A_, schema, …) a first-class concept, and
[ADR-0010](0010-keyboard-status-domain-and-salience.md) gave each mode a
`salience` that tells the host whether to auto-reveal it on incidental focus.
In the in-process model an engine pushed mode changes synchronously through
`typio_instance_notify_keyboard_mode`, and the host kept its indicator and tray
in lock-step.

[ADR-0015](0015-ipc-only-engine-backend.md) moved every engine out of process.
A worker cannot call back into the framework, the line protocol had no channel
for an unsolicited mode change, and `process-key` replies reported only the key
result. The framework therefore stopped reflecting mode at all: the host's mode
indicator and tray icon froze at their initial state, and `salience` was dropped
on the wire (`write_mode_line` serialised `is_active` but not `salience`). Two
distinct precision losses, one root cause — no path from a worker's internal
mode to the host.

A keyboard worker is **host-driven**: its internal mode only changes as a side
effect of a request the framework sent (a key, a mode restore, a reset). There
is no spontaneous keyboard mode change. So the reply to that request is the
canonical, race-free place to carry the resulting mode — no async side-channel
and no per-key polling are required, and engine authors keep writing a simple
single-threaded read/respond loop.

## Decision

1. **Reply-carried mode.** Any worker reply MAY include a trailing
   `ACTIVE_MODE` line giving the engine's current active mode after the request
   was applied. Engines emit it from requests that can change the mode —
   `process-key`, `set-active-mode`, `reset`, `focus-in`.

2. **`salience` on the wire.** `MODE` and `ACTIVE_MODE` lines gain a trailing
   integer field after `is_active`: `0` = quiet, `1` = notable. It is optional
   for forward/backward compatibility; an omitted field is read as quiet ("the
   default is silence").

3. **Framework reconciliation.** The IPC backend caches the last reported active
   mode per worker and exposes a transition through
   `KeyboardEngine::take_changed_mode`. After every mode-affecting request the
   framework drains it and, on a change, synthesises the host notification that
   the in-process model used to deliver — the host stays unchanged.

4. **Deliberate vs incidental.** A mode change from `process-key` is a
   *deliberate* user action and fires the host's `mode_changed_callback`, which
   always confirms it. A change observed from a host-driven request
   (`set-active-mode`, `reset`, `focus-in`) is *incidental*: it refreshes
   `last_mode` and the persistent status icon but does **not** fire the
   deliberate callback, leaving the host's focus path to apply its own
   salience/recency gate (see [ADR-0010](0010-keyboard-status-domain-and-salience.md)).

## Alternatives considered

- **Poll `get-active-mode` after every key**: Rejected. An extra round-trip per
  keystroke for data the reply could have carried, and it still can not observe
  changes that are not key-driven.
- **A second, asynchronous worker→host event channel**: Rejected. It forces
  every engine author to add concurrency (a thread or event loop to emit events
  between requests) to handle a case that cannot occur for host-driven keyboard
  engines, and it complicates the single-pipe line framing.
- **Leave mode reflection to a future redesign**: Rejected. The indicator/tray
  regression is user-visible today and the protocol already carried the mode
  metadata for `list-modes`/`get-active-mode`; only the change notification and
  one wire field were missing.

## Consequences

- Positive: Indicator and tray track the engine's internal mode again, with the
  same precision as the in-process model and no host changes.
- Positive: `salience` survives the process boundary, so the host's on-focus
  auto-reveal policy is honoured.
- Positive: Engines stay single-threaded; the reply is the only carrier.
- Trade-off: A mode-reporting engine writes one extra `ACTIVE_MODE` line per
  mode-affecting request; the framework de-duplicates by identity so the host
  is notified only on real transitions.
- Negative (accepted): Engines that report mode must serialise the new
  `salience` field; the trailing field is optional, so older workers keep
  parsing but are treated as quiet.
