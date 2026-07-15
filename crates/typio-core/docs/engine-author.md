# Engine Author Path

Curated reading order for anyone authoring a Typio **engine**: an
out-of-process `typio-engine-<name>` engine process declared by a
`typio-engine-<name>.toml` manifest.

## What an engine is

An executable engine process that:

- Includes engine headers from `typio/abi/` (umbrella: `typio/abi/abi.h`),
  plus `typio/schema/config_schema.h` only when publishing config fields.
- Speaks Typio Engine Protocol on fd 3.
- May use the C engine ABI internally to implement lifecycle and modality
  operations.
- Implements `TypioEngineBaseOps` (lifecycle) plus the modality-specific vtable (`TypioKeyboardEngineOps` or `TypioVoiceEngineOps`).
- Lives in its own repository and releases independently of libtypio.

The engine never talks to Wayland, never schedules paints, never knows about other engines. It is a pure function from key/audio events to text deltas.

## Reading order

### 1. Orient — what an engine is and is not

1. [Architecture Overview](explanation/architecture-overview.md) — where engines sit in the ecosystem.
2. [Engine Contract](explanation/engine-contract.md) — the boundary between framework and engine, two lifecycles (instance vs. composition), the property bag, the dual-vtable design. **Required reading.**
3. [Composition State Machine](explanation/composition-state-machine.md) — preedit + candidates as one transactional state, commit as a separate event ([ADR-0006](adr/0006-composition-state-and-commit-event.md)).

### 2. Rules you must follow

1. [Contract Layers](dev/contract-layers.md) — engines consume `typio/abi/`,
   may declare fields through `typio/schema/config_schema.h`, and never include
   `typio/runtime/`.
2. [Engine Naming Convention](dev/engine-naming-convention.md) — mandatory
   rules for repository, executable, manifest, and runtime names.
3. [ADR-0003: Plugin Engine ABI — dual-category slots](adr/0003-plugin-engine-abi-dual-category.md) — why keyboard and voice are distinct C types.

### 3. Build something that runs

Pick the modality you're building.

**Keyboard engine**

1. [How to Create a Custom Keyboard Engine](how-to/create-custom-keyboard-engine.md) — minimal complete example, `Cargo.toml`, install steps.
   - If writing the engine in **Rust**, depend on the [`typio-abi`](../../README.md#header-layers) crate for shared `#[repr(C)]` types rather than copying them by hand.
2. [How to Integrate a Keyboard Engine](how-to/integrate-keyboard-engine.md) — packaging, verification, config-schema registration.

**Voice engine**

1. [How to Create a Custom Voice Engine](how-to/create-custom-voice-engine.md) — minimal complete example.
   - If writing the engine in **Rust**, depend on the [`typio-abi`](../../README.md#header-layers) crate for shared `#[repr(C)]` types rather than copying them by hand.
2. [How to Integrate a Voice Engine](how-to/integrate-voice-engine.md) — packaging and verification.
3. [Voice Input Architecture](explanation/voice-input.md) — VAD, audio buffering, idle policy.

### 4. Engine reference

After the minimal example runs, look up exact signatures here.

1. [Engine ▸ index](reference/engine/index.md) — catalog, ABI-version, and capability-negotiation rules.
2. [Entry points](reference/engine/entry.md) — required exports, define-macros, lifecycle helpers.
3. [Types](reference/engine/types.md) — `TypioEngineInfo`, `TypioEngine`, `TypioKeyboardEngine`, `TypioVoiceEngine`, capability names.
4. [Operations](reference/engine/ops.md) — `TypioEngineBaseOps`, `TypioKeyboardEngineOps`, `TypioVoiceEngineOps`.

### 5. Configuration and assets

1. [Engine Reference](reference/engines.md) — existing engines and their config sections; a template for documenting yours.
2. [Configuration Reference](reference/configuration.md) — full `core.toml` shape.
3. [Engine Icons](reference/engine/icons.md) — icon resolution rules.

### 6. When it does not work

Report issues in the engine's own repository. For `libtypio`-side problems (registration failures, ABI mismatches), see [Contributing](../CONTRIBUTING.md).

## After this you should be able to

- Write a `typio-engine-<name>` worker and matching `.toml` manifest.
- Implement all seven base callbacks plus the modality vtable correctly, distinguishing engine-instance lifecycle from composition lifecycle.
- Emit composition (preedit + candidates) and commit events through the `TypioInputContext` correctly.
- Persist per-context engine state in the property bag rather than in globals or context-coupled fields.
- Declare required and optional capabilities so the host can accept or reject your engine at load time.

## See also

- [Host Integrator Path](host-integrator.md) — what the host on the other side of the ABI does.
- [Glossary](reference/glossary.md).
- [Contributing](../CONTRIBUTING.md) — only if contributing back to libtypio itself; engines live in their own repositories.
