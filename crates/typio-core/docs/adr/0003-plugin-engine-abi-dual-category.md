# ADR-0003: Plugin Engine ABI with Dual-Category Slots

- **Status**: Accepted
- **Date**: 2026-05-28
- **Deciders**: Project maintainers

## Context

`libtypio` is a framework host, not a language engine. The actual input logic (Rime, Mozc, speech recognition, future handwriting and emoji engines) must live behind a stable boundary so engines can be developed, built, and shipped independently of the core library — including as out-of-tree shared objects published by third parties.

Two unrelated input modalities coexist: a primary **keyboard** pipeline (composition, candidates, commit) and a secondary **voice** pipeline (speech → text). They are selected independently and must not interfere with each other.

## Decision

Engines are loaded through a small C ABI ([ADR-0002](0002-c-abi-as-the-only-public-interface.md)) and managed in two parallel slots.

- An engine shared object exports `typio_engine_get_info` plus a type-specific factory: `typio_keyboard_engine_create` for keyboard engines or `typio_voice_engine_create` for voice engines.
- `TypioEngineInfo` opens with a `struct_size` sentinel so the struct can grow without breaking already-built plugins. Readers honor only the fields the writer's size covers.
- `TypioRegistry` holds exactly one active **keyboard** engine and one active **voice** engine. Selecting within one category never evicts the other.
- Engine instances are created lazily on first activation. If creation or activation fails, the registry restores the previously active engine in the same category.
- Engines own their `user_data` and all engine-specific runtime state (schema, mode, learning dictionaries); the host owns protocol hosting and UI.
- Common lifecycle fields live in `TypioEngine`. `TypioKeyboardEngine` and `TypioVoiceEngine` both embed `TypioEngine` as their first member and append their modality-specific vtable pointer. A pointer to either specific type can be safely cast to `TypioEngine *` and back.

The published `typio-engine-abi.pc` pkg-config file installs the ABI headers so out-of-tree plugins discover them via `dependency('typio-engine-abi')`. A single umbrella header `typio/abi/abi.h` gives engine authors the entire ABI in one include and nothing host-only.

## Alternatives considered

- **Single flat engine list.** Rejected: keyboard and voice would share one active slot and evict each other, contradicting running dictation alongside typing.
- **Compiled-in engines only.** Rejected: prevents third-party and out-of-tree engines and couples engine release cadence to the core library.
- **Fixed-size info struct.** Rejected: would prevent additive evolution; the leading `struct_size` sentinel makes that safe.
- **Single `TypioEngine` struct with both `keyboard` and `voice` pointers.** Rejected: mixed modalities in one type, forced callers to pass NULL arguments, and encouraged implicit runtime-type-based routing. The split structs give true compile-time type safety and symmetric APIs.

## Consequences

- Positive: engines are independently buildable and loadable; the framework stays language-agnostic.
- Positive: keyboard and voice evolve and fail independently.
- Positive: the C API is fully symmetric — `register_keyboard` / `register_voice`, `set_active_keyboard` / `set_active_voice`, `next_keyboard` / `next_voice`, …
- Trade-off: two-category management is more complex than one active engine.
- Negative (accepted): cross-version ABI care is now a permanent obligation — fields are append-only and gated by `struct_size`.

## Related

- [ADR-0004: Platform-neutral core, host-owned loading](0004-platform-neutral-core-host-loading.md) — how the host (not core) discovers and loads engine `.so` files
- [ADR-0005: Internal engine backend abstraction](0005-internal-engine-backend-abstraction.md) — how the public C ABI maps onto the internal Rust trait model
