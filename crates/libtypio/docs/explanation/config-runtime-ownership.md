# Config & Runtime Ownership

This document defines the ownership model for Typio's four state domains: persisted config, runtime state, local staged edits, and view state.

It exists because Typio now has multiple control surfaces, multiple engine types, and a growing set of settings that influence both persisted config and runtime behaviour. The main failure mode is allowing those layers to drift until one UI starts guessing state that actually belongs to the daemon.

## Goal

Every user-visible state value in Typio must belong to exactly one of these domains:

- `persisted config` — canonical user intent stored in owner-specific config files
- `runtime state` — current daemon state exposed over the status bus
- `local staged edits` — client-side edits not yet confirmed by the daemon
- `view state` — temporary widget state with no authority of its own

The design target is simple:

- each config owner owns its persistent config file; the daemon owns runtime truth
- control surfaces may stage edits, but may not invent truth
- runtime and config must never be silently substituted for each other

## Ownership Rules

### Persisted Config

Owned by the process boundary that owns the file. Libtypio owns
`core.toml`; the Wayland frontend owns `wayland.toml`.

Rules:

- `core.toml` owns framework policy, shortcuts, voice runtime, and per-engine settings
- frontend-specific files such as `wayland.toml` own frontend presentation settings
- only the owning process writes its file
- control surfaces mutate config through the owning process API, such as `SetConfigText`
- config defaults happen owner-side on initial load, explicit reload, and accepted edits

Examples:

- `shortcuts.voice_ptt`
- `notifications.enable`
- `keyboard.per_app_preferences`
- `display.panel_theme`

### Runtime State

Owned by the daemon and published over the D-Bus status interface.

Rules:

- runtime state must be consumable without reading local files
- control surfaces must display runtime state from D-Bus, not by inferring from config
- runtime state may differ temporarily from persisted config during startup, reload, failure, or async engine switching
- when an engine switch fails, runtime state should remain on the previously active engine if it can be restored; persisted intent is not silently changed

Current runtime properties include:

- `ActiveKeyboardEngine`
- `ActiveEngine`
- `AvailableKeyboardEngines`
- `ActiveVoiceEngine`
- `AvailableVoiceEngines`
- `AvailableEngines`
- `OrderedKeyboardEngines`
- `ActiveEngineState`
- `RimeSchema`
- `ConfigText`

### Local Staged Edits

Owned by a single client instance such as `typio-settings`.

Rules:

- local stage starts from daemon `ConfigText`
- local stage may diverge from daemon state while an autosave is pending
- stale daemon replies must not overwrite newer staged edits
- local stage is disposable; it is not a source of truth outside that client

### View State

Owned by the widget tree.

Rules:

- widget state must be reconstructed from runtime state and staged config
- widget state must never be written back unless the corresponding domain is known
- unknown state must remain unknown; do not guess the first dropdown item

## Binding Rules

Each control should bind to exactly one authority for reads and one authority for writes.

Patterns:

- **runtime selector** — read from D-Bus runtime property, write to daemon method or config edit path
- **config editor** — read from staged `ConfigText`, write back to staged `ConfigText`
- **mixed selector** — show runtime truth in the control, but stage persistent intent separately

Examples:

- keyboard engine selector — read runtime from `ActiveKeyboardEngine`, write through `ActivateEngine`; the daemon persists the `[keyboard]` section of `engine-state.toml`
- voice backend selector — read runtime from `ActiveVoiceEngine`, write through the voice activation API; the daemon persists the `[voice]` section of `engine-state.toml`
- Rime schema selector — read runtime selection from `RimeSchema`, persist via `SetRimeSchema`

## Control-State Bindings

`typio-settings` now has a small binding layer for high-risk selector state.

Current `ControlStateBinding` responsibilities:

- identify the authoritative key or runtime property
- identify the widget that renders the state
- map string values to dropdown indices and back
- declare a `value source policy`
- optionally provide an `options refresh` callback

Current value-source policies:

- `FROM_CONFIG` — read from staged daemon `ConfigText`
- `FROM_RUNTIME` — read from the daemon runtime property; persistence, if any, is handled by the runtime action
- `RUNTIME_THEN_CONFIG` — prefer daemon runtime state, fall back to staged config when runtime is temporarily unavailable

Current selector mapping in `typio-settings`:

- keyboard engine — state-driven last-used engine, value source `FROM_RUNTIME`, options source `OrderedKeyboardEngines`
- voice backend — state-driven last-used voice, value source `RUNTIME_THEN_CONFIG`, options source `AvailableVoiceEngines`
- Rime schema — exposed through the engine's generic surface (`TypioEngineSurfaceOps::list_properties` returns a `schema` property whose choices are the installed schemas; see `typio/abi/engine.h`).

This binding layer exists to prevent control-surface code from repeating manual logic for choosing between runtime and config state, guessing fallback selections, and re-implementing the same dropdown selection rules in multiple places.

New selector-style state should prefer extending this layer rather than adding new ad-hoc sync code in `control_config.c`.

## Schema Guidance

The config schema table is the registry for persisted fields. It should also carry enough metadata to tell control surfaces whether a field has a direct runtime counterpart.

Current rule: fields with a direct runtime mirror should set `runtime_property`.

Current examples:

- last-used engine state -> `ActiveKeyboardEngine`
- last-used voice state -> `ActiveVoiceEngine`

This is intentionally narrow. Do not attach a runtime property unless there is one stable daemon property that represents the field's live value.

## Reload And Rollback Rules

Config reload is a daemon-owned boundary:

1. parse the replacement config
2. apply schema defaults
3. switch affected runtime engines
4. refresh runtime subsystems through the frontend callback
5. publish updated runtime/config state over D-Bus

Rules:

- empty or invalid replacement config is rejected before defaults are applied
- missing config files are treated as an empty user config plus schema defaults
- if a requested keyboard or voice engine cannot be created or activated, the engine manager restores the previous active engine in that category when possible
- keyboard and voice rollback are independent
- voice reload may complete after the config reload boundary if recording or inference is currently active

## Best-Practice Workflow For New State

When adding a new stateful feature:

1. decide whether it is persisted, runtime, or both
2. if persisted, add it to `config_schema.rs`
3. if runtime, expose it through the status bus
4. if a persisted key has a direct runtime mirror, set `runtime_property`
5. make control surfaces read from the correct authority
6. add regression tests for startup, refresh, and delayed apply cases

For selector-style state such as engine choice, voice backend, and Rime schema, the minimum regression bar is:

- `options -> UI` — refreshed options must include any explicit unselected state and preserve a configured value even if discovery is incomplete
- `config -> UI` — loading staged config must select the matching option in the widget
- `UI -> config` — user selection must write the correct persisted key, including removing the key when the selector is explicitly unselected

For runtime-driven selectors such as last-used engine, also require:

- `runtime/status -> UI` — the status bus must export the runtime value and the control surface must map that runtime value back to the correct internal selector id, not a display label

Treat selector tests as belonging to one of three categories:

- `config-driven` — example: `display.panel_theme`
- `runtime-driven` — example: last-used engine
- `mixed` — example: last-used voice

Do not reuse a config-selector test pattern for a runtime selector just because both render as dropdowns. The test contract must match the selector's declared `ControlStateValueSource`.

## Anti-Patterns

Do not:

- read `core.toml` directly from a UI
- infer runtime state from config when a daemon property exists
- infer config state from current runtime activation
- write config from widget defaults before the first daemon seed
- replace unknown state with the first available dropdown option
- maintain separate ad-hoc lists of runtime property names in different layers
- add selector UIs without tests that cover `options -> UI`, `config -> UI`, and `UI -> config`

## Migration Plan

Short-term:

- centralize D-Bus property and method names in shared protocol headers
- annotate persisted config fields that have direct runtime mirrors
- keep expanding regression coverage around startup and async apply

Mid-term:

- add a small runtime-state registry beside the config schema if runtime properties become numerous
- move control-surface bindings toward schema-driven and protocol-driven tables instead of one-off handwritten mappings

Long-term:

- make new control surfaces consume the same state model without bespoke state ownership rules
