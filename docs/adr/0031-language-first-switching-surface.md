---
id: ADR-0031
title: "Language-first switching surface (`language.*`, `languages` manifest key)"
status: accepted
date: 2026-06-12
scope: control
---

# ADR-0031: Language-first switching surface (`language.*`, `languages` manifest key)

- **Status**: Accepted
- **Date**: 2026-06-12
- **Deciders**: Project maintainers

## Context

libtypio ADR-0018 makes **language** (a BCP-47 tag) the user-facing switch
unit: engines declare supported languages, the registry owns an
active-language slot, and activating a language re-resolves the keyboard and
voice slots together. A language with no keyboard engine is **layout-only**:
the keyboard slot is deactivated and keys pass through raw (e.g. Moroccan
Darija on an Arabic layout).

The host currently exposes engine-level switching everywhere:

- The Ctrl+Shift chord (key arbiter) calls `typio_registry_next_keyboard`.
- The tray `activate`/scroll actions cycle keyboard engines.
- TIP v2 has modality verbs (`keyboard.*` / `voice.*`, ADR-0026) but no
  language namespace.
- Engine manifests declare a single optional `language` tag that nothing
  consumes.

## Decision

Adopt the language model across every host control surface in one wave, with
engine cycling as the explicit fallback when no languages exist.

### Manifest

`typio-engine-*.toml` gains a repeatable `languages` key (TOML array,
ordered, primary first; `"mul"` declares every language). After
`typio_registry_register_engine_process` succeeds, the loader forwards the
list via `typio_registry_set_engine_languages`. The single `language` key
remains accepted as the primary tag for manifests that predate this ADR;
`languages` wins when both are present.

### Shortcut

The cached binding `switch_engine` becomes `switch_language`, loaded from the
`switch_language` action (default Ctrl+Shift, owned by libtypio). The key
arbiter's chord consume calls `typio_registry_next_language`; when it returns
`TYPIO_ERROR_NOT_FOUND` (no languages enabled or declared) the arbiter falls
back to `typio_registry_next_keyboard`, preserving today's behavior on
installations without language metadata. `switch_keyboard_engine` stays a
recognized action with no default chord.

### IPC (TIP v3)

New `language.*` namespace, mirroring the registry API:

```text
language.list  {}      -> { languages: [{ tag, active }], active }
language.use   { tag } -> {}
language.next  {}      -> { active }
language.prev  {}      -> { active }
```

Per-language engine selection needs no new verbs: it is config
(`languages.<tag>.keyboard` / `languages.<tag>.voice` via `config.set`).
`daemon.status` gains `activeLanguage`. A new `language.changed` event
carries `{ activeLanguage, activeKeyboardEngine, activeVoiceEngine }`; the
state controller derives it by diffing the registry's active language on
every engine-change notification, so it fires for layout-only switches too.
`hello` bumps `protocolVersion` to `3` and advertises the `language`
capability namespace. Per the ADR-0008 no-shim mandate there are no aliases;
`keyboard.*` / `voice.*` keep their ADR-0026 semantics as raw slot
operations and do not change the active language.

### Tray

The tray `activate` and scroll actions cycle languages with the same
engine-cycling fallback as the chord. The menu's engine list keeps selecting
keyboard engines directly (a within-language choice).

### Startup

libtypio's `typio_instance_init` restores the persisted language
(ADR-0018) before the legacy `keyboard.engine` / `voice.engine` chain; the
host adds nothing.

## Alternatives considered

- **Host-side language table instead of libtypio APIs.** Rejected: activation
  rules are libtypio's domain (project scope); a host table would duplicate
  resolution in every surface and drift from `typioctl` / `typio-settings`.
- **`language.engines.set { tag, kind, name }` verb.** Rejected: the mapping
  is plain config; `config.set languages.<tag>.<kind>` already round-trips
  through schema, events, and persistence.
- **Replace `keyboard.next` / `voice.next` with language verbs.** Rejected:
  modality slot operations remain the substrate (ADR-0026) and stay useful
  for cycling engines within a language.
- **Synthesize a "passthrough" engine for layout-only languages.** Rejected
  in ADR-0018: an empty keyboard slot already means passthrough; a fake
  engine would pollute `engine.list`, the tray, and state files.

## Consequences

- Positive: Ctrl+Shift means the same thing everywhere — next language —
  and retargets keyboard and voice together.
- Positive: layout-only languages work with zero engines installed.
- Positive: installations without language metadata behave exactly as
  before via the engine-cycling fallback.
- Trade-off: TIP bumps to v3; `typioctl` and `typio-settings` adopt
  `language.*` in the same wave (no shims).
- Trade-off: requires libtypio >= 0.4.0 (language registry API); the wrap
  pin and CI pin move together with that release.
- Negative (accepted): `language.changed` is derived from engine-change
  notifications plus a registry poll rather than a dedicated libtypio
  callback; if a future libtypio gains one, the derivation collapses into it.

## Related

- libtypio ADR-0018 — language model, resolution rules, persistence.
- [ADR-0026](0026-modality-explicit-engine-control-surface.md) — modality
  verbs that remain the substrate.
- [ADR-0030](0030-engine-process-manifests.md) — manifest schema this ADR
  extends.
- [Engine Discovery Reference](../reference/engine-discovery.md) — manifest
  key table.
- [IPC Protocol Reference](../reference/ipc-protocol.md) — TIP v3 catalog.
