## ADR-0009: Engine status reflection — engagement axis + open active profile

- **Status**: Superseded
- **Date**: 2026-05-31
- **Deciders**: Project maintainers
- **Superseded by**: [ADR-0011](0011-engine-mode-as-first-class-concept.md)

## Context

`TypioEngineMode` (types.h) is how an engine *reflects* its live observable state
to the framework, which drives the tray icon, the candidate-bar mode indicator,
and the `engine.modeChanged` IPC topic. Its original shape was:

```c
typedef enum { TYPIO_MODE_CLASS_NATIVE = 0, TYPIO_MODE_CLASS_LATIN = 1 } TypioModeClass;

typedef struct TypioEngineMode {
    TypioModeClass mode_class;
    const char *mode_id;
    const char *display_label;
    const char *icon_name;
} TypioEngineMode;
```

This conflated two genuinely orthogonal things into a single closed 2-value enum,
and it surfaced concretely with Rime:

1. **`mode_class` (NATIVE/LATIN) is two axes pretending to be one.** Whether the
   engine is *composing vs passing input through* (Rime `ascii_mode`, Mozc
   `direct`) is one axis. *Which layout/profile is active* (Rime **schema**: 拼音
   / 五笔 / 仓颉; Mozc input mode) is a different axis. A user can be "拼音 +
   中文", "拼音 + 英文", "五笔 + 中文". Folding these into NATIVE/LATIN loses the
   profile entirely; adding a third enum value per schema is a forced taxonomy
   over an open, user-defined set.

2. **The naming is keyboard-script-centric.** "LATIN" is meaningless for a voice
   engine (whose engagement is listening/idle/off). The only axis that
   generalises across engine *types* is "is the engine actively transforming
   input, passing through, or off".

3. **Schema switches were invisible.** librime emits a `"schema"` notification on
   switch, but nothing mapped it onto a mode update, and the dedup in
   `engine_mode_equal` keyed on `(mode_class, mode_id)` — so two schemas reported
   as `mode_id="chinese"` were collapsed and the change never propagated.

This is the *reflection* counterpart to ADR-0008, which made the config-schema
layer the single home for engine property **control** (e.g. `engines.rime.schema`
is read/written there). ADR-0008 governs *changing* the active profile; this ADR
governs *observing* it.

## Decision

Model engine status as **two orthogonal axes**, plus presentation that rides
alongside. Identity (for change detection) is `(engagement, profile_id)`.

```c
/* Universal, closed axis — the framework interprets this. */
typedef enum {
    TYPIO_ENGAGE_ACTIVE = 0,      /* composing in the engine's native script */
    TYPIO_ENGAGE_PASSTHROUGH = 1, /* input passes through (Rime ascii_mode, Mozc direct) */
    TYPIO_ENGAGE_OFF = 2,         /* present but not transforming input */
} TypioEngagement;

typedef struct TypioEngineMode {
    TypioEngagement engagement;   /* universal engagement axis */
    const char *profile_id;       /* active profile id, engine-defined (e.g. Rime schema) */
    const char *profile_label;    /* human profile name, e.g. "朙月拼音" (may be NULL) */
    const char *display_label;    /* short UI badge ("中" / "A" / "あ") */
    const char *icon_name;        /* freedesktop icon name */
} TypioEngineMode;
```

- **`engagement`** is closed and small. The framework reasons about it generically
  (indicator styling, "is the IME hot"). It replaces `mode_class`; ASCII
  passthrough maps to `PASSTHROUGH`, not to a "Latin" script value.
- **`profile_id` / `profile_label`** are the engine-defined **active profile** — an
  open set the framework only *displays*. Switching profiles is a *control*
  concern owned by the config-schema layer (ADR-0008); this struct only reflects
  whatever the engine currently reports.
- **`engine_mode_equal` compares `(engagement, profile_id)`.** Both axes therefore
  propagate: toggling ascii flips `engagement`; switching schema changes
  `profile_id`. Presentation fields (`display_label`, `icon_name`) ride along and
  are not part of identity.

### The icon carries engagement, not profile

The tray icon reflects **engagement only**. An engine ships exactly the icons for
its own engagement states (Rime: composing vs ascii passthrough). The framework
deliberately does **not** map the open profile set onto per-profile icons: that
would require a user to ship an SVG for every custom schema — pushing an asset
burden onto them and producing broken icons for any schema without one. The
active profile is conveyed by its **name** (`profile_label` → tooltip / panel /
candidate bar), which every user-defined profile inherently has.

A schema switch while staying composing (ACTIVE → ACTIVE, `profile_id` changes)
therefore still fires `modeChanged` and refreshes the *name*, while the tray
glyph stays the generic composing icon. icon ← engagement; label ← profile.

### Bidirectional consistency

When a profile changes through the engine's own UI (e.g. Rime's F4 schema menu)
rather than through a config write, the engine mirrors the new `profile_id` back
into the config tree (`engines.rime.schema`) so the canonical state, the panel,
and persistence never drift. An equality guard (write only when the value
actually differs) terminates the config-set → on_config_change → select → notify
loop.

## Alternatives considered

- **Keep `mode_class` NATIVE/LATIN, add schema as a third enum value.** Rejected:
  schemas are an open, user-defined set; enumerating them is a forced taxonomy,
  and it still conflates the engagement and profile axes.
- **Per-engine opaque status extension (a blob the host treats as a black box).**
  Rejected: it fragments the framework — indicator styling, tooltip, D-Bus, and
  the panel's profile switcher would each need per-engine logic, defeating the
  point of a unified host (and contradicting ADR-0008).
- **Per-profile tray icons by naming convention + fallback.** Rejected as an
  asset-burden overreach: distinctiveness for an open profile set belongs on the
  text/label channel, not on icons the user would have to author.
- **Carry both `display_label` and `profile_label` as one field.** Rejected: the
  short engagement badge ("中"/"A") and the profile name ("朙月拼音") are distinct
  consumers (candidate bar vs tooltip) and should not be overloaded.

## Consequences

- Positive: the abstraction is honest and engine-type-neutral; schema switches
  propagate correctly; no per-schema asset burden; control (ADR-0008) and
  reflection (this ADR) are cleanly separated and linked by `profile_id`.
- Trade-off: switching between two composing profiles does not change the tray
  glyph (only the name/label updates). This is intended.
- Negative (accepted): a breaking ABI change to `TypioEngineMode`. All keyboard
  engines and the host were migrated in lockstep; the `engine.modeChanged` IPC
  payload changed from `{ modeClass, modeId, … }` to
  `{ engagement, profileId, profileLabel, displayLabel, iconName }`.
- This change is independent of the engine ABI **version** mechanism
  (version.h / `typio_engine_abi_version`); both landed together but address
  different concerns (status shape vs plugin/host compatibility negotiation).
