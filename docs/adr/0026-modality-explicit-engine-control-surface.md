# ADR-0026: Modality-explicit engine control surface (`keyboard.*` / `voice.*`)

- **Status**: Accepted
- **Date**: 2026-06-05
- **Deciders**: Project maintainers

## Context

The framework already separates input modality almost everywhere:

- **ABI** (libtypio): distinct types `TypioKeyboardEngine` / `TypioVoiceEngine`,
  distinct ops vtables, distinct factories, distinct `TYPIO_*_ENGINE_DEFINE`
  macros. The engine header states the framework never mixes them in one slot.
- **Runtime registry** (libtypio): two orthogonal active slots —
  `active_keyboard` and `active_voice` — with kind-specific API
  (`set_active_keyboard` / `set_active_voice`, and full symmetric cycling:
  `next_keyboard` / `prev_keyboard` / `next_voice` / `prev_voice`). A keyboard
  engine and a voice engine are **simultaneously active**, not alternatives in
  one slot.

The IPC control surface (ADR-0008) collapses this back into a kind-agnostic
`engine.*` namespace:

```text
engine.use   { name }        -> {}          # kind inferred from the engine's declared type
engine.next  { kind? }       -> { active }   # kind OPTIONAL → "next of what?" when omitted
engine.list  {}              -> [{ name, kind, displayName, active }]
```

`engine.list` already carries `kind` per entry, and `daemon.status` already
reports `activeKeyboardEngine` and `activeVoiceEngine` separately. So the model
underneath is split; only the external **verbs** remain ambiguous. `engine.next`
with no `kind` has no well-defined meaning, and `engine.use` routes through an
implicit name→kind inference that the registry API does not need.

## Decision

Promote modality to first-class IPC resource namespaces, mirroring the registry
API one-to-one. The guiding principle: **`engine` is a shared implementation
substrate (packaging, discovery, lifecycle, registry slot); wherever a human or
external client acts on an engine, the modality is explicit.**

```text
keyboard.use   { name }   -> {}
keyboard.next  {}         -> { active }
keyboard.prev  {}         -> { active }
voice.use      { name }   -> {}
voice.next     {}         -> { active }
voice.prev     {}         -> { active }
```

`engine.*` keeps only genuinely cross-modality, name-keyed operations:

```text
engine.list      {}                       -> [{ name, kind, displayName, active }]   # aggregate; unchanged
engine.describe  { name }                 -> { name, kind, displayName, properties, commands }
engine.invoke    { name, command, args? } -> { result }
```

This **amends ADR-0008**: the ambiguous `engine.use` and `engine.next {kind?}`
are removed. No optional-kind inference remains anywhere on the surface.

Discovery and packaging stay modality-unified: one `libtypio_engine_<name>.so`,
one search path, kind discovered after `typio_engine_get_info()` (see
[ADR-0025](0025-engine-discovery-search-path.md)). Modality is explicit where
clients act, implicit where it is only substrate.

Events (`engine.changed`, `engine.statusChanged`) carry `kind` in the payload
rather than splitting into `keyboard.changed` / `voice.changed`; subscribers
filter on the field. Splitting topics is deferred until a subscriber needs
server-side topic filtering.

The `hello` handshake bumps `protocolVersion` to `2` and adds `keyboard` and
`voice` to the advertised capability namespaces, so a client detects the new
contract on connect. Per the ADR-0008 no-shim mandate there are no
`engine.use` / `engine.next` aliases; `typioctl` and `typio-settings` move to
the new verbs in the same wave.

## Alternatives considered

- **Keep `engine.*`; make `kind` a *required* parameter on `use`/`next`.**
  Rejected. It still funnels every action through one verb plus a
  discriminator. The registry already exposes kind-specific operations, so
  namespaced verbs map 1:1 and read better in handshakes and client SDKs —
  consistent with ADR-0008's own "resource namespaces are an asset" rationale.
- **Split the whole plugin ABI into two unrelated contracts.** Rejected. The
  shared `TypioEngine` base buys one packaging story, one discovery path, one
  lifecycle contract, and one ABI-version witness. Duplicating those is pure
  cost; modality differs only in the ops vtable and backend transport, which
  are already separate.
- **Split discovery too (`libtypio_keyboard_*` / `libtypio_voice_*`, or per-kind
  directories).** Rejected. Kind is known after `get_info()`; encoding it in
  the filename or directory adds packaging complexity to save one `dlopen` of
  the wrong kind, and engines are few.
- **Split event topics into `keyboard.changed` / `voice.changed`.** Deferred:
  a `kind` field in the payload is sufficient today.

## Consequences

- Positive: the "next of what?" ambiguity is gone; CLI and IPC verbs map 1:1 to
  the registry's kind-specific API.
- Positive: clients can drive keyboard and voice independently — which is the
  reality (both active at once) that the kind-agnostic surface obscured.
- Positive: `engine.*` shrinks to true aggregates (`list` / `describe` /
  `invoke`), a cleaner ontology.
- Trade-off: this amends an accepted protocol (ADR-0008); `typioctl` and
  `typio-settings` port to the new verbs in the same wave, with no shims.
- Negative (accepted), open dependency: engine-name uniqueness scope must be
  decided. Kind-namespaced verbs would *allow* a keyboard and a voice engine to
  share a name, but the registry's single by-name `slots` collection and the
  `engines.<name>.*` config namespace currently assume global uniqueness.
  Recommendation: keep global uniqueness initially; defer re-keying on
  `(kind, name)` until a real collision appears.

## Related

- [ADR-0008](0008-ipc-protocol-resource-namespaces-uds-only.md) — engine
  control verbs amended here.
- [ADR-0025](0025-engine-discovery-search-path.md) — discovery stays
  modality-unified; this ADR governs only the control surface.
- libtypio runtime registry API (`set_active_keyboard` / `set_active_voice`,
  `next_*` / `prev_*`) — the model these verbs mirror.
