# ADR-0014: Engine availability as a first-class lifecycle axis

- **Status**: Accepted
- **Date**: 2026-06-03
- **Deciders**: Project maintainers

## Context

An input engine has a lifecycle moment where it exists and is the active
engine, but cannot yet process input: a keyboard engine doing asynchronous
warm-up (Rime deploying schemas, which can take tens of seconds on first run),
a voice engine loading a model, an engine rebuilding a session after config
reload. Today there is **no way to express this for keyboard engines**.

The gap surfaced concretely with Rime. When a field is focused the instant the
user starts typing, `transition_to_active` focuses the context and immediately
marks the host phase ACTIVE — it does **not** wait for Rime's background deploy.
Rime's `get_session` returns NULL while deploying, so `process_key` returns
`TYPIO_KEY_NOT_HANDLED`. The host reads "not handled" as "the engine does not
want this key" and forwards it raw to the application, then latches the key to
its repeat path — producing stuck, raw key-repeat that bypasses the IME
entirely. The engine *did* want the key; it just was not ready, and there was no
vocabulary to say so.

`NOT_HANDLED` is the wrong channel for "not ready": it conflates *"pass this
through, it is not mine"* with *"this is mine but I cannot serve it yet"*. The
`TypioKeyProcessResult` enum has no value for the latter, and
`TypioKeyboardEngineOps` has no readiness predicate.

Voice engines already acknowledge this axis, but in a keyboard-blind,
under-powered form: a bare `is_ready()` predicate on `TypioVoiceEngineOps`
(no reason, no transitions, no observability). The concept was modelled per engine *kind* instead
of where it belongs — on every engine — and could not be observed, only polled.

Availability is genuinely orthogonal to the existing engine-status axes:

| Axis | Question | Owner |
| --- | --- | --- |
| **Availability** (this ADR) | Can the engine work at all? | Engine declares |
| **Engagement** (ADR-0009/0010) | Is it transforming input right now? | Engine declares |
| **Mode / Profile** (ADR-0011) | Which script / schema? | Engine declares |

## Decision

Introduce **engine availability** as a first-class lifecycle axis defined on the
**base** engine (every engine kind), surfaced through both a pull query and a
push notification, and gated by the host before input routing.

### 1. A closed availability state enum

```c
typedef enum {
    TYPIO_ENGINE_UNINITIALIZED = 0, /* Created, init() not yet completed. */
    TYPIO_ENGINE_PREPARING,         /* Async warm-up in progress — NOT routable. */
    TYPIO_ENGINE_READY,             /* Can process input — routable. */
    TYPIO_ENGINE_FAILED,            /* Warm-up failed — NOT routable; host falls back. */
} TypioEngineAvailability;
```

A NUL-terminated, human-readable `reason` (e.g. `"Deploying schemas…"`)
accompanies a state so the host indicator can be honest. The reason is optional
(may be NULL) and borrowed for the duration of the notification.

### 2. Pull: a base-ops query

`TypioEngineBaseOps` gains:

```c
TypioEngineAvailability (*availability)(TypioEngine *engine);
```

Optional — **NULL means the engine is always `READY`**, so the overwhelming
majority of engines (single-mode keyboard engines with no async warm-up) need
no change. The pull form fits consumers that sample on demand: the voice
capture loop checks availability each chunk.

### 3. Push: an instance notification + host observer

Mirroring the mode-changed mechanism (ADR-0011), the engine pushes transitions
so the host reacts the instant the engine becomes ready — without polling and
without waiting for the next keystroke:

```c
/* engine → framework */
void typio_instance_notify_engine_availability(TypioInstance *instance,
                                               TypioEngineAvailability state,
                                               const char *reason);

/* framework → host */
typedef void (*TypioEngineAvailabilityChangedCallback)(
    TypioInstance *instance, TypioEngineAvailability state,
    const char *reason, void *user_data);
void typio_instance_set_engine_availability_changed_callback(...);

/* host pull of the cached value (initial sync / fallback) */
TypioEngineAvailability typio_instance_get_engine_availability(TypioInstance *instance);
```

The instance caches the last availability, de-duplicates, and fans out — exactly
as `typio_instance_notify_keyboard_mode` does for modes.

### 4. The host gates routing on availability; the engine never decides key policy

When the active engine is not `READY`, the host **must not** call `process_key`
and **must not** forward the key raw to the application. The engine only
*declares* availability; the host *owns* what happens to keys during warm-up.
This matches the separation already codified for salience (ADR-0010: engine
classifies meaning, host owns timing).

Host policy during a non-`READY` window (normative default, host-tunable):

- **Sub-perceptual warm-up** (a bounded short window): buffer the raw key events
  and replay them through the normal routing path once `READY`. No data loss.
- **Long warm-up** (window elapsed): drop the buffer and freeze routing behind
  an honest "engine preparing" indicator, so the user knows not to type yet.

Gating before `process_key` means the engine never has to defend against being
called while not ready, and raw keys can never leak.

### 5. Fold voice readiness into this axis

`TypioVoiceEngineOps.is_ready` and the Rust `VoiceEngine::is_ready` are
**removed**. Voice engines report through the base `availability` op
(`READY`/`PREPARING`), and the voice session derives readiness as
`availability() == TYPIO_ENGINE_READY`. The voice `is_ready` special-case is
deleted in favour of the one general concept.

## Alternatives considered

- **`TYPIO_KEY_NOT_READY` result on `process_key`**: per-key, minimal ABI delta,
  but recovery can only happen on the *next* keystroke — a user who types a
  burst and stops sees the keyboard frozen until they press again. No push, no
  reason, and every engine must remember to return it on every code path.
  Rejected as the primary mechanism; the gate-before-`process_key` design makes
  it unnecessary.
- **Keep `is_ready()` as a bare predicate, add one for keyboard too**: carries no
  reason, no transition semantics, and is poll-only — reproducing the voice
  weakness on the keyboard side. Rejected.
- **Don't grab the keyboard until READY**: during warm-up the compositor would
  route keys to the application raw — the very leak we are fixing. The host must
  grab early and gate. Rejected.
- **Buffer-and-replay unconditionally**: replaying pinyin typed blind several
  seconds ago into a now-ready engine is a worse surprise than a clear
  "preparing" signal. Rejected in favour of the bounded-window split.

## Consequences

- Positive: keyboard engines can warm up asynchronously without leaking raw keys
  or sticking key-repeat; the host recovers the instant the engine is ready.
- Positive: one availability concept across keyboard and voice; the voice
  `is_ready` special-case is removed.
- Positive: availability is observable (push) and honest (carries a reason),
  enabling a real "engine preparing" host indicator.
- Trade-off: `process_key` MUST NOT be invoked while not `READY` — a new host
  obligation at the routing boundary.
- Negative (accepted): removing `TypioVoiceEngineOps.is_ready` is a breaking ABI
  change for voice engines; in-tree engines (sherpa) migrate to the base op.
