## ADR-0011: Engine Mode as a first-class framework concept

- **Status**: Accepted
- **Date**: 2026-06-02
- **Deciders**: Project maintainers
- **Supersedes**: [ADR-0009](0009-engine-status-reflection-engagement-and-active-profile.md), [ADR-0010](0010-keyboard-status-domain-and-salience.md)

## Context

ADR-0009 modelled keyboard engine status as two axes: **engagement** (Active /
Passthrough / Off) and **profile** (engine-defined identifier).  ADR-0010
refined the naming to be keyboard-domain-honest.  Both ADRs treated engagement
as the primary axis and profile as a ride-along field.

Experience with real engines reveals a missing layer:

1. **Engagement is not the toggle; mode is.** When a user presses Shift in
   Rime, they toggle *ascii_mode ↔ native mode*. Engagement (Passthrough ↔
   Active) was modelled as a **consequence** of that mode change, but exposing
   it as a first-class framework axis turned out to be redundant: the engine
   already expresses whether it handles a key via `process_key` return values,
   and the host already decides whether an engine is active via the registry.

2. **Mode is a user-facing concept; engagement was a routing concern that
   leaked into the wrong layer.** Users think "I'm in English mode" or
   "I'm in browse mode", not "my engine's engagement changed to Passthrough".
   The indicator should show the mode name. Routing is entirely the engine's
   business: the host sends all keys to the active engine and lets it decide.

3. **No standard mode-switch UX.** Every engine handles mode switching
   differently. Users get inconsistent experiences: Rime has its own trigger,
   basic engine has Multi_key, and no two engines share a mechanism. A standard
   "cycle mode" trigger (e.g. Shift) should work identically across all engines.

4. **Profile is necessary.** Rime schema switching (pinyin ↔ wubi) is not a
   mode change in the same sense as ascii_mode, but users need to see which
   schema is active. The mode struct must carry an open profile field so the
   framework can surface it in UI without conflating it with the mode identity.

### What we need

- **Mode**: a named, user-facing engine state (e.g. `native`, `ascii`,
  `normal`, `browse`). The engine declares its modes and notifies the
  framework when the active mode changes.
- **Profile**: an engine-defined active profile (e.g. Rime schema) carried
  alongside the mode for display purposes. Not part of mode identity.
- **Standard mode trigger**: a host-level mechanism (e.g. lone Shift press)
  that engines can use for mode cycling. The host detects the trigger pattern;
  the engine decides what mode to switch to.

## Decision

### 1. Replace `TypioKeyboardEngineStatus` with `TypioKeyboardEngineMode`

The old status struct is renamed and restructured. Mode is the primary identity;
profile is a display field that rides alongside. Engagement is removed.

```c
typedef struct TypioKeyboardEngineMode {
    const char             *id;             /* "native", "ascii", "normal", "browse" */
    const char             *label;          /* "Native", "ASCII", "Normal", "Browse" */
    const char             *display_label;  /* Short indicator badge: "中", "A", "Browse" */
    const char             *icon_name;      /* freedesktop icon name */

    /* Profile — engine-defined active profile (e.g. Rime schema). */
    const char             *profile_id;     /* "luna_pinyin", "wubi86" */
    const char             *profile_label;  /* "朙月拼音", "五笔86" */
    const char             *description;    /* Optional detailed description */

    TypioStatusSalience    salience;        /* Announcement salience ceiling */
} TypioKeyboardEngineMode;
```

Identity for change detection is `id` only. All other fields are presentation /
metadata that ride alongside.

### 2. Replace `get_status` / `set_status` with mode-oriented ops

```c
typedef struct TypioKeyboardEngineOps {
    /* existing */
    TypioKeyProcessResult (*process_key)(TypioKeyboardEngine *, TypioInputContext *, const void *);

    /* mode surface — engine declares its modes */
    const TypioKeyboardEngineMode *(*list_modes)(TypioKeyboardEngine *engine, size_t *count);
    const TypioKeyboardEngineMode *(*get_active_mode)(TypioKeyboardEngine *engine, TypioInputContext *ctx);
    TypioResult (*set_active_mode)(TypioKeyboardEngine *engine, TypioInputContext *ctx, const char *mode_id);
} TypioKeyboardEngineOps;
```

- **`list_modes`**: returns a static array of all modes the engine supports.
  The host uses this to know available modes, display them in UI, and determine
  cycling order. Returns `NULL` with `count = 0` for engines with no mode
  concept (a single-mode engine is equivalent to no mode surface).
- **`get_active_mode`**: returns the currently active mode. The host calls this
  on focus-in and mode change to determine UI state (indicator, tray, panel).
  The host routes all keys to the active engine regardless of mode; the engine
  decides via `process_key` whether to consume or pass through each key.
- **`set_active_mode`**: host requests a mode switch by ID. The engine may
  accept or reject. Called when the standard mode trigger fires or when the user
  selects a mode from UI. Passing `NULL` as `mode_id` means "cycle to next
  mode".

### 3. Mode notification

```c
/* Engine → framework */
void typio_instance_notify_keyboard_mode(TypioInstance *instance,
                                        const TypioKeyboardEngineMode *mode);
void typio_instance_clear_keyboard_mode(TypioInstance *instance);

/* Framework → host observer */
typedef void (*TypioKeyboardModeChangedCallback)(
    TypioInstance *instance,
    const TypioKeyboardEngineMode *mode,
    void *user_data);

void typio_instance_set_keyboard_mode_changed_callback(
    TypioInstance *instance,
    TypioKeyboardModeChangedCallback callback,
    void *user_data);

const TypioKeyboardEngineMode *typio_instance_get_last_keyboard_mode(
    TypioInstance *instance);
```

These replace the old `typio_instance_notify_keyboard_status` /
`typio_instance_clear_keyboard_status` /
`typio_instance_set_keyboard_status_changed_callback` /
`typio_instance_get_last_keyboard_status`.

### 4. Standard mode trigger

The host detects a **lone Shift press-release** (Shift pressed and released
without any other key in between) and sends a synthetic key event to the
engine via `process_key`. The engine recognises this pattern internally and
decides whether to cycle modes.

This is deliberately not an ABI concept — the trigger mechanism is between the
host (detects the pattern) and the engine (interprets it). The framework only
provides the mode notification mechanism above.

Engine-side Shift detection:
- Track Shift press via `process_key` (keysym == Shift, type == KeyPress).
- If the next event is Shift release without an intervening non-Shift key
  press, it's a "lone Shift" — toggle to next mode.
- Any non-Shift key press between Shift down and up resets the detection.

### 5. Profile is part of the mode struct

ADR-0009's `profile_id` / `profile_label` are reintroduced as fields on the
mode struct. Profile switching (e.g., Rime schema: pinyin ↔ wubi) is surfaced
in UI via `profile_label`, while `mode->label` remains the mode name. The two
are distinct: a user can be in "Native" mode with "朙月拼音" profile, or in
"Native" mode with "五笔" profile.

### 6. Engagement is removed

`TypioKeyboardEngagement` and all host-side routing based on it are deleted.
Routing decisions are simplified:

- If a keyboard engine is active, all keys go to it.
- The engine returns `NOT_HANDLED`, `HANDLED`, `COMPOSING`, or `COMMITTED`.
- If the host wants to bypass the engine entirely, it deactivates the engine
  via the registry — it does not ask the engine to report an `OFF` mode.

## Alternatives considered

- **Keep engagement as the primary axis (ADR-0009 status quo).** Rejected:
  engagement is a routing property, not a user-facing state. Users toggle
  modes, not engagement levels. Moreover, the engine's `process_key` return
  value already carries the same semantic.

- **Add mode as a third axis alongside engagement and profile.** Rejected:
  three axes in one struct is over-engineered. Mode subsumes the need for
  profile in the status struct.

- **Mode trigger as a framework ABI concept.** Rejected: the trigger mechanism
  is a host↔engine convention (lone Shift). Formalising it in the ABI would
  limit future trigger mechanisms. The engine's `process_key` already receives
  all key events; it can detect any trigger pattern internally.

- **Generic "engine command" shortcut instead of Shift.** Deferred: the engine
  command surface (`invoke_command`) already exists and can be used in the
  future for configurable shortcuts. Shift is the zero-config default.

## Consequences

- Positive: Mode is a first-class, user-facing concept. Profile is surfaced
  for display. Engagement is removed, eliminating the redundant routing axis.
  Users get consistent mode-switch UX (Shift → indicator shows mode name)
  across all engines. Engine developers have a clear contract: declare modes
  → notify on change → host handles the rest.
- Trade-off: breaking ABI change — all keyboard engines and the host must be
  rebuilt. The `TypioKeyboardEngineStatus` struct, `get_status`/`set_status`
  ops, and notification functions are all replaced. This is a greenfield
  rewrite with no backward compatibility.
- Negative (accepted): Shift detection adds subtle timing/state tracking to
  every engine that wants mode cycling. This could be extracted into a shared
  helper function in a future `typio-engine-utils` crate.
