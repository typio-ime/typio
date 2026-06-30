# Keyboard status salience — when to ask for an announcement

This is the engine-author guide for the `salience` field of
`TypioKeyboardEngineMode`. It applies to **keyboard engines** only; voice and
other modalities do not report this field (see
[ADR-0011](../adr/0011-engine-mode-as-first-class-concept.md)).

## What salience is

`salience` answers one question, and only this one:

> **When the user *incidentally focuses* a text field (no deliberate change),
> should the host auto-reveal that they are in this state?**

A useful proxy: *if the user starts typing right now without looking, could the
result surprise them?*

- **No** → `TYPIO_STATUS_SALIENCE_QUIET`
- **Yes** → `TYPIO_STATUS_SALIENCE_NOTABLE`

It describes the **resulting state**, not the size of the transition that
reached it. It is *not* a request to show any particular widget — the host
decides whether and how to surface it.

### What salience does NOT govern

Salience controls only the **unprompted on-focus auto-reveal**. A *deliberate,
user-initiated change* — switching engine or changing mode — **always** earns
confirmation feedback in the host, regardless of salience. The user just acted;
they deserve the clearest signal. So a `QUIET` state still announces when the
user deliberately switches *into* it (e.g. toggling to ASCII shows a brief "A");
`QUIET` only means "don't pop up unprompted when I merely focus a field already
in this state."

## The contract: you set a ceiling, the host may only lower it

```
engine salience  ──►  host policy  ──►  shown / suppressed
   (meaning)          (environment)
```

- The **engine** owns *meaning*: only you know that your Latin transliteration
  profile is unsurprising while CJK composing is not.
- The **host** owns *environment*: focus churn, how recently the user engaged,
  whether a candidate panel is already up, and user config.

On focus, the host may **suppress** a `NOTABLE` state (e.g. a terminal that
re-focuses on every click), but it will **never auto-reveal** a `QUIET` one. So
on the focus path `QUIET` is a hard silence guarantee; `NOTABLE` is "auto-reveal
*if the environment agrees*." (Deliberate changes always announce — see above.)

## The default is silence

`TYPIO_STATUS_SALIENCE_QUIET` is `0`. An engine that never sets the field — or
never reports mode at all (returns `NULL` from `get_active_mode`) — produces no
unprompted announcements. **Opt in to noise; never opt out.** If your engine
behaves like a plain keyboard, do nothing and you are correct.

## Rules of thumb

| State | Salience | Why |
|-------|----------|-----|
| Latin / ASCII / direct / passthrough | `QUIET` | What you type is what you get. |
| Engine off / not transforming | `QUIET` | Behaves like no IME. |
| Composing native non-Latin script (pinyin, kana, hangul) | `NOTABLE` | Keystrokes do not map 1:1 to glyphs. |
| Profile switch within one script family (e.g. 拼音 → 双拼) | `NOTABLE` | The *state* is still notable; the host's recency rule, not you, damps repeats. |
| A profile that emits Latin and matches user expectation | `QUIET` | Override the default downward. |

## Do / Don't

- **Do** classify the *destination state*, not the *act of switching*.
- **Do** use `QUIET` for anything that behaves like the user's home keyboard.
- **Do** override the default in either direction when the universal guess is
  wrong for your engine.
- **Don't** manage frequency, timing, or de-duplication — that is the host's
  job. Report the honest salience of the current state every time.
- **Don't** assume a specific UI (indicator, popup, tray). You express intent;
  the host renders.
- **Don't** use `NOTABLE` as "important" in some other sense. It means
  *surprising to type into blind* — nothing more.

## Worked example: Rime

`rime_fill_mode` (`typio-engine-rime/src/rime_mode.c`):

```c
buf->mode.salience = ascii ? TYPIO_STATUS_SALIENCE_QUIET
                           : TYPIO_STATUS_SALIENCE_NOTABLE;
```

`ascii_mode` types Latin → `QUIET`; composing any schema → `NOTABLE`. Switching
schema (拼音 → 五笔) stays `NOTABLE`; the host suppresses back-to-back repeats by
recency, so the engine never has to reason about it.

## On the wire (out-of-process engines)

A worker serialises `salience` as the **trailing integer** of every `MODE` and
`ACTIVE_MODE` line, after `is_active`: `0` = `QUIET`, `1` = `NOTABLE`. The field
is optional for compatibility — an omitted value is read as `QUIET`, preserving
"the default is silence".

Because a worker has no callback into the framework, it reports a mode change by
appending an `ACTIVE_MODE` line to the reply of the request that caused it
(`process-key`, `set-active-mode`, `reset`, `focus-in`). The framework
de-duplicates by identity and turns a real transition into the host
notification. A change from `process-key` is *deliberate* (always confirmed); a
change from a host-driven request is *incidental* (refreshes state but leaves the
on-focus salience gate to the host). See
[ADR-0016](../adr/0016-out-of-process-active-mode-reflection.md).

## See also

- [ADR-0011: Engine Mode as a first-class framework concept](../adr/0011-engine-mode-as-first-class-concept.md)
- [ADR-0016: Out-of-process active-mode reflection](../adr/0016-out-of-process-active-mode-reflection.md)
- [Engine ABI stability](abi-stability.md)
