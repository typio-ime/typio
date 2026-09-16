# ADR-0018: Language-first switching — language as the user-facing switch unit

- **Status**: Accepted
- **Date**: 2026-06-12
- **Deciders**: Typio maintainers

## Context

The registry switches engines per modality: `active_keyboard` and
`active_voice` slots with kind-specific cycling (`switch_keyboard`,
`switch_voice`). The user-facing switch chord (Ctrl+Shift) cycles keyboard
engines directly.

This puts the wrong unit in front of the user:

- Users think "type Chinese now", not "activate rime". Every mainstream
  platform (Windows Win+Space, macOS input sources, fcitx5 groups) switches
  by language and binds engines per language underneath.
- Keyboard and voice slots drift: switching the keyboard to a Chinese engine
  leaves the voice slot on an English recognizer. The two slots are
  simultaneously active (ADR-0003) but there is no concept that retargets
  them together.
- Some languages need no engine at all. A layout-only language (for example
  Moroccan Darija typed on an Arabic layout) is simply "no keyboard engine,
  forward keys raw". The engine-cycling model cannot express it: there is no
  engine to cycle to.
- Future engine kinds (handwriting, hardware dictation) would each add
  another disconnected switch surface.

Engines already declare one `language` tag in `EngineInfo`; nothing consumes
it for switching.

## Decision

Introduce **language** (a BCP-47 tag) as the user-facing switch unit, layered
above the modality slots:

1. **Engines declare supported languages.** `EngineInfo` gains an ordered
   `languages` list (primary first; empty means "exactly `language`"; the
   pseudo-tag `mul` declares every language). Out-of-process engines declare
   the list in their manifest; hosts forward it via
   `typio_registry_set_engine_languages` after registration. The engine ABI
   struct is untouched — the list is runtime registry metadata, not an ABI
   field.
2. **The registry owns one active-language slot.** `activate_language(tag)`
   re-resolves and retargets every modality slot, atomically from the user's
   point of view. Per modality the engine is chosen by:
   1. config override `languages.<tag>.<modality>` — the string `none`
      forces an empty slot; an unregistered override falls back with a
      warning;
   2. the first registered engine of that modality declaring a matching
      language (case-insensitive equality, `mul` wildcard, or primary-subtag
      prefix at a `-` boundary: `zh` serves `zh-Hans` and vice versa);
   3. otherwise the slot is **deactivated**. For keyboards this yields raw
      passthrough — the contract for layout-only languages.
3. **The cycle is the `languages.enabled` config list** (order = cycle
   order), falling back to every engine-declared language in registration
   order. `cycle_language` is a pure step; activation failures deactivate
   the affected slot instead of aborting the switch.
4. **Ctrl+Shift becomes `switch_language`.** The new shortcut action takes
   the default chord. `switch_keyboard_engine` stays a recognized action for
   engine cycling within a language but loses its built-in default.
5. **Persistence**: the active language is stored in `engine-state.toml`
   (`[language] active`); `typio_registry_restore_language` reactivates it
   (or the first enabled language) at host startup.
6. The kind-specific slot API (`set_active_keyboard`, `switch_keyboard`,
   `set_active_voice`, …) remains the substrate, unchanged. Direct engine
   selection within the active language stays possible and does not change
   the active language.

New C surface (runtime header, no ABI stability promise):
`typio_registry_set_engine_languages`, `typio_registry_get_engine_languages`,
`typio_registry_list_languages`, `typio_registry_get_active_language`,
`typio_registry_set_active_language`, `typio_registry_next_language`,
`typio_registry_prev_language`, `typio_registry_restore_language`.

## Alternatives considered

- **Keep engine cycling and group engines client-side (per host or settings
  UI).** Rejected: every surface (chord, tray, IPC, settings) would
  re-implement the same grouping with drifting semantics; the registry is the
  single owner of activation rules.
- **Add `languages` to the `TypioEngineInfo` ABI struct.** Rejected: the
  struct is allocated by whichever side declares it, so appending a field is
  an incompatible layout change forcing an ABI major bump. The manifest is
  the real source of the list since ADR-0017; a runtime registry call
  carries it without touching the ABI.
- **A language is a (keyboard, voice) engine pair with no resolution.**
  Rejected: it cannot express layout-only languages, multilingual voice
  engines (`mul`), or sensible defaults when only one engine declares a
  language; every install would need explicit per-language config.
- **Model layout-only languages as a built-in "passthrough" engine.**
  Rejected: a fake engine would appear in `engine.list`, tray menus, and
  state files. An empty keyboard slot already means passthrough in the
  input path; reusing it keeps the ontology honest.

## Consequences

- Positive: one switch gesture retargets keyboard and voice together; the
  user-facing unit matches platform conventions.
- Positive: layout-only languages (Darija case) work with zero engines, via
  slot deactivation → raw passthrough.
- Positive: future engine kinds plug into language resolution by adding a
  modality slot, not a new switch surface.
- Trade-off: hosts must forward manifest language lists and call
  `restore_language` at startup; until they do, language switching reports
  `NotFound` and hosts fall back to engine cycling.
- Trade-off: `switch_keyboard_engine` loses its default chord; users who
  relied on Ctrl+Shift engine cycling now cycle languages. Engine cycling
  remains available by binding the action explicitly.
- Negative (accepted): per-modality activation during a language switch is
  best-effort — a failing engine deactivates its slot rather than failing
  the switch, so a language switch can land with an empty keyboard slot.
- Negative (accepted): no instance-level "language changed" callback yet;
  hosts that initiate the switch already know, and modality callbacks still
  fire. A callback can be added when an external trigger needs it.

## Related

- [ADR-0003](0003-plugin-engine-abi-dual-category.md) — the modality slots
  this model layers on.
- [ADR-0017](0017-typio-engine-protocol.md) — manifests as the metadata
  source for out-of-process engines.
- Host-side control surface and manifest key: `typio-linux` ADR-0031.
