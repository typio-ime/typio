## ADR-0010: Keyboard status is keyboard-domain; announcement salience

- **Status**: Superseded
- **Date**: 2026-06-01
- **Deciders**: Project maintainers
- **Refines**: [ADR-0009](0009-engine-status-reflection-engagement-and-active-profile.md)
- **Superseded by**: [ADR-0011](0011-engine-mode-as-first-class-concept.md)

## Context

ADR-0009 introduced the two-axis status reflection `(engagement, profile_id)`
and argued the engagement axis was the *universal, engine-type-neutral* one —
"the only axis that generalises across engine types". Experience showed that
claim was over-abstraction:

1. **The type names lied about scope.** `TypioEngagement` and
   `TypioEngineStatus` are produced only by `TypioKeyboardEngineOps.get_status`
   and consumed only via `typio_instance_notify_status`. No voice engine ever
   touches them — `TypioVoiceEngineOps` has no status concept at all. Yet the
   names and the "universal" framing imply every engine type should implement
   this model.

2. **The vocabulary is keyboard semantics.** `PASSTHROUGH` means *key*
   passthrough (Rime `ascii_mode`, Mozc direct). A voice engine has no "audio
   passthrough"; its natural states are idle/listening/processing. `profile_id`
   (schema / input mode) is likewise keyboard-shaped. Forcing voice (or future
   handwriting) status into the engagement enum would be the same kind of
   forced taxonomy ADR-0009 itself rejected for schemas.

Separately, the host needed a way to decide *when an engine state is worth an
unprompted announcement* (the on-focus indicator). Only the engine knows the
*meaning* of a state — a Latin transliteration profile is unsurprising while CJK
composing is not — but only the host knows the *environment* (focus churn,
recency, candidate UI, user config). Putting the whole decision on either side
is wrong: a host-only rule cannot read engine semantics, and an engine that
commands "show the indicator" reaches across into UI concepts it must not know
and bypasses the host's anti-noise suppression.

## Decision

**1. Name the keyboard status surface honestly.** The status-reflection types
are keyboard-domain, not universal:

| Before | After |
|--------|-------|
| `TypioEngagement` | `TypioKeyboardEngagement` |
| `TYPIO_ENGAGE_{ACTIVE,PASSTHROUGH,OFF}` | `TYPIO_KB_ENGAGE_{…}` |
| `TypioEngineStatus` | `TypioKeyboardEngineStatus` |
| `TypioStatusChangedCallback` | `TypioKeyboardStatusChangedCallback` |
| `typio_instance_notify_status` | `typio_instance_notify_keyboard_status` |
| `typio_instance_clear_status` | `typio_instance_clear_keyboard_status` |
| `typio_instance_get_last_status` | `typio_instance_get_last_keyboard_status` |
| `typio_instance_set_status_changed_callback` | `typio_instance_set_keyboard_status_changed_callback` |

The two-axis `(engagement, profile_id)` model of ADR-0009 is unchanged; only its
*scope* is corrected. A future voice modality that needs status reflection gets
its own shape (e.g. `TypioVoiceEngineStatus { IDLE, LISTENING, PROCESSING }`),
not an overload of keyboard engagement. The `engine.statusChanged` IPC topic
keeps its wire name (it is an external contract; the IPC layer may present
keyboard status as "engine status" to clients).

**2. Add an announcement-salience ceiling to keyboard status.** A new field
expresses one thing only: should this state *auto-reveal* on an **incidental
focus** (the host's on-focus indicator)? It does **not** govern deliberate,
user-initiated transitions — an engine switch, a profile change, or an
engagement toggle always earns confirmation feedback in the host regardless of
salience, because the user just acted and deserves the clearest signal. Salience
narrows only the *unprompted* reveal.

```c
typedef enum {
    TYPIO_STATUS_SALIENCE_QUIET   = 0, /* home-keyboard-like; never announce unprompted */
    TYPIO_STATUS_SALIENCE_NOTABLE = 1, /* could surprise if typed into blind; worth a brief announce */
} TypioStatusSalience;

typedef struct TypioKeyboardEngineStatus {
    TypioKeyboardEngagement engagement;
    const char *profile_id;
    const char *profile_label;
    const char *display_label;
    const char *icon_name;
    TypioStatusSalience salience;   /* NEW: announcement-salience ceiling */
} TypioKeyboardEngineStatus;
```

The contract is a one-way valve: **the engine sets a ceiling; the host may only
lower salience (suppress), never raise it.** The engine classifies *meaning*;
the host owns *when/whether* to surface it (recency, focus churn, candidate UI,
config). `QUIET = 0` is the zero default, so an engine that sets nothing — or a
plugin built against an older minor — stays silent. Salience rides alongside the
presentation fields and is **not** part of `(engagement, profile_id)` identity.

Engine-author guidance lives in
[docs/dev/keyboard-status-salience.md](../dev/keyboard-status-salience.md).

**3. ABI versioning.** This is a backward-compatible addition (a field appended
to a keyboard-only struct plus renamed symbols rebuilt in lockstep), carried by
bumping `TYPIO_ENGINE_ABI_MINOR` 1 → 2.

## Alternatives considered

- **Keep the "universal" framing; document that voice ignores it.** Rejected:
  the names still mislead new engine authors and the comment would contradict
  the type name.
- **A boolean `announce` instead of a salience enum.** Rejected: "announce"
  leaks a UI verb into a UI-neutral ABI, and a bool cannot grow a middle ground.
- **Let the engine command the indicator directly.** Rejected: violates the
  ABI's UI-neutrality and disables the host's environmental suppression
  (the terminal click-to-focus noise problem).
- **Derive salience entirely in the host from `engagement`.** Rejected: the host
  cannot distinguish a Latin transliteration profile (ACTIVE but unsurprising)
  from CJK composing. The host keeps an engagement-derived *default* but the
  engine must be able to override it.

## Consequences

- Positive: names match scope; voice authors are no longer implicitly told to
  implement engagement; the host gains an engine-sourced silence guarantee while
  retaining its own anti-noise policy.
- Trade-off: a cross-repo rename (host + every keyboard engine) plus a minor ABI
  bump; all built in lockstep per [abi-stability](../dev/abi-stability.md).
- Negative (accepted): appending to a host-read, engine-filled struct relies on
  engines being rebuilt against the new headers — consistent with the pre-1.0
  "rebuild against the headers you target" rule. A stale plugin reporting minor 1
  is still accepted by the minor check, so do not over-read salience from such a
  plugin; in practice all engines rebuild together.
