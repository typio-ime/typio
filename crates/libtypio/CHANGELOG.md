# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0] - 2026-06-23

### Added

- **Per-language "last-used engine" memory.** The registry now remembers
  which keyboard engine the user last chose for each language and reuses
  it when that language is activated again, instead of always falling
  back to the first engine in registration order. The memory is persisted
  in `engine-state.toml` (`[language-engines]`) and survives restarts. It
  sits below an explicit `languages.<tag>.keyboard` config override and
  above registration order in `resolve_language_engine`'s precedence, and
  is keyboard-only (voice engines picked independently never populate it).

### Changed

- **`cycle_language` returns `None` for fewer than two enabled languages.**
  A single (or empty) language list has nothing to cycle; hosts now get a
  clean signal to fall back to engine cycling.

### Fixed

- **The active language now tracks the active keyboard engine.** Switching
  keyboards directly (tray submenu, `typioctl keyboard use`, hotkeys)
  previously left the registry's `active_language` stale, so the indicator
  popup and tray-icon badge kept showing the previous language — e.g. the
  badge stayed `EN` after activating a Chinese engine. `activate_keyboard`
  now reconciles `active_language` with the newly active engine's declared
  language (BCP-47 matched; no-op within the same language; `und`/`mul`
  engines leave it untouched).

## [0.5.0] - 2026-06-23

### Added

- **Rust-native `TypioInstance` API.** Five `pub` methods callable from
  any Rust host (typio-linux ADR-0035) without going through `extern "C"`
  wrappers:

  - `TypioInstance::new_rust(config_dir, data_dir, state_dir, engine_dirs)
    -> Box<Self>` — typed constructor mirroring
    `typio_instance_new_with_config` but taking `Option<&str>` and
    `Vec<String>` instead of `*const TypioInstanceConfig`.
  - `instance.init_rust() -> Result<(), TypioResult>` — initialiser
    returning a typed Result instead of the C `TypioResult` int.
  - `instance.shutdown_rust()` — typed shutdown.
  - `instance.registry_rust() -> Option<&EngineRegistry>` — typed
    accessor returning a direct reference to the native Rust registry
    (no `*mut TypioRegistry` dereference at the call site).
  - `instance.registry_rust_mut() -> Option<&mut EngineRegistry>` —
    mutable counterpart of `registry_rust`, for hosts that need to
    drive the registry (engine registration, slot switching, state
    mutation) without going through the C ABI.
  - `instance.config_rust() -> Option<&Config>` — typed accessor for
    the config tree.

  The C ABI surface (`typio_instance_new` / `_init` / `_free` / etc.)
  is unchanged — engine plugins and other C consumers see no
  difference. The Rust methods are thin wrappers over the existing
  `pub(crate)` helpers, so behaviour is identical.

  This unblocks the typio-linux Rust host port from going through
  `extern "C"` wrappers for any step of the instance lifecycle.

### Changed

- **Removed `EngineRegistry::set_instance(*mut TypioInstance)` from the
  native Rust API.** The method was a no-op vapor: `InstanceHandle` is
  a zero-sized placeholder struct with no methods, so the raw pointer
  was discarded on the floor. The C ABI's `TypioRegistry` keeps its
  own back-pointer to `TypioInstance` (in `c_api/registry.rs`) for its
  own callback plumbing; that field is unaffected and remains the sole
  legitimate `*mut TypioInstance` storage. The change matters because
  `EngineRegistry::set_instance` was the only place a `*mut
  TypioInstance` leaked into the supposedly native `core::*` API, and
  its presence was blocking the Rust host port (typio-linux ADR-0035)
  from constructing a registry without first constructing a
  `TypioInstance`. `InstanceHandle::from_raw` is also removed; the
  `InstanceHandle` struct and `Engine::init(&mut self, &mut
  InstanceHandle)` signature are kept as a future-extension point.

## [0.4.2] - 2026-06-19

### Added

- New shortcut action ID `summon_indicator` (default `Ctrl+Super+i`).
  Hosts use it to actively re-show the on-screen indicator (language ·
  engine · mode) on demand, instead of only on focus/engine-change
  triggers. The default chord avoids the common system/WM/app conflicts
  — `Super+<letter>` is claimed by window managers, `Ctrl+Space` by
  IBus/Fcitx5. Three-modifier chords (Ctrl+Super+<key>) are essentially
  unused by GNOME/KDE.

## [0.4.1] - 2026-06-19

### Fixed

- Developer docs (`docs/dev/project-layout.md`) now match the actual source
  tree: removed stale references to `cbindgen`, `meson.build`,
  `c_api/plugin_adapter.rs`, `core/registry/scheduler.rs`, and
  `voice/idle.rs`; documented the `Process` backend as the only engine
  transport; expanded the ecosystem table to cover every consumer repo.
- `CONTRIBUTING.md` quick-start now uses `cargo build` / `cargo test`
  (the meson wrapper was removed in an earlier release).
- CI workflow repaired: the previous `ci.yml` invoked `meson setup` and
  `working-directory: core`, neither of which exists; every job would
  have failed. The new workflow builds and tests via cargo, including an
  ASan job under nightly.
- `typio_registry_list_ordered_keyboards` docstring now states plainly
  that `engine_order` is not yet honored and the function is currently
  equivalent to `typio_registry_list_keyboards`.

## [0.4.0] - 2026-06-13

### Added

- **Language-first switching** ([ADR-0018](docs/adr/0018-language-first-switching.md)).
  Language (BCP-47 tag) is the user-facing switch unit. Engines declare an
  ordered `languages` list (forwarded by hosts via
  `typio_registry_set_engine_languages`); the registry owns an
  active-language slot whose activation re-resolves every modality slot
  through `languages.<tag>.keyboard` / `languages.<tag>.voice` config
  overrides, declared-language matching (`mul` wildcard, primary-subtag
  prefix), or slot deactivation — an empty keyboard slot is raw passthrough
  for layout-only languages. New C API:
  `typio_registry_{set,get}_engine_languages`,
  `typio_registry_list_languages`,
  `typio_registry_{get,set}_active_language`,
  `typio_registry_{next,prev}_language`, `typio_registry_restore_language`.
  The active language persists in `engine-state.toml`.

### Changed

- The default Ctrl+Shift chord now belongs to the new `switch_language`
  shortcut action. `switch_keyboard_engine` remains recognized but has no
  built-in default; bind it via `shortcuts.switch_keyboard_engine`.

## [0.3.0] - 2026-06-10

### Added

- **Typio Engine Protocol** ([ADR-0017](docs/adr/0017-typio-engine-protocol.md)).
  Engine processes now communicate with libtypio over a private fd 3 channel
  using bounded, versioned frames. Standard output and standard error are
  reserved for logs.
- **Out-of-process active-mode reflection** ([ADR-0016](docs/adr/0016-out-of-process-active-mode-reflection.md)).
  The process backend caches the active keyboard mode reported in an engine response
  and exposes a transition through `KeyboardEngine::take_changed_mode`. The
  framework drains it after every mode-affecting request and synthesises the
  host's `mode_changed_callback` — restoring indicator/tray mode precision that
  was lost when engines moved out of process. A change driven by `process-key`
  is announced (deliberate); one observed on focus/restore refreshes state
  silently (incidental).
- `EngineMode::salience`, parsed from the trailing field of `MODE` /
  `ACTIVE_MODE` engine response lines (`0` quiet, `1` notable; optional, defaults quiet),
  so the host's on-focus auto-reveal policy survives the process boundary.

### Changed

- Renamed the host registration API to `typio_registry_register_engine_process`.
  Public names now describe the runtime purpose; IPC remains an implementation
  detail of the engine protocol transport.

### Fixed

- Retry engine process spawn on `ETXTBSY` (bounded, with backoff). An engine binary that
  was just written and marked executable — or one being replaced by a package
  upgrade — can still be open for writing when `execve`'d; the condition is
  transient. Other spawn errors still fail fast.

## [0.2.0] - 2026-06-06

### Changed

- **Engine backends are out-of-process only.** Removed the in-process C plugin adapter and
  plugin registration APIs. Hosts now register engines with
  `typio_registry_register_engine_process`, passing engine argv supplied by their
  own discovery layer.

- **Engine author documentation now follows the worker package layout.**
  Engine packages install private workers under
  `<libexecdir>/typio/engines` and manifests under
  `<datadir>/typio/engines`.

## [0.1.7] - 2026-06-05

### Fixed

- **Sync Rust ABI version to match C header (MINOR=2).** The C header
  (`version.h`) defined `MINOR=2` after adding the `availability` field for
  ADR-0014, but the Rust `typio-abi` crate still had `MINOR=1`. This mismatch
  caused struct layout divergence where the host expected 9 function pointers
  but plugins only provided 8, leading to SIGSEGV when the host called
  `base.init()` through garbage. All engines must be rebuilt against the
  updated `typio-abi` crate.

## [0.1.6] - 2026-06-05

### Changed

- **CPluginAdapter sandboxing for third-party engine fault isolation.**
  Every C plugin vtable call (`init`, `deactivate`, `focus_in`, `focus_out`,
  `reset`, `reload_config`, `availability`, `process_key`, `get_active_mode`,
  `set_active_mode`, `commit_candidate`, `process_audio`) is now wrapped in
  `std::panic::catch_unwind` via `sandbox_call`. If a third-party plugin
  panics, the daemon logs a warning and returns a safe fallback instead of
  crashing. Note: this catches Rust panics only; C-side SIGSEGV/SIGABRT
  cannot be caught without out-of-process hosting.

## [0.1.5] - 2026-06-03

### Changed

- **Engine availability is now a first-class base lifecycle axis (ADR-0014).**
  Added `TypioEngineAvailability`, `TypioEngineBaseOps::availability`,
  `typio_instance_notify_engine_availability`, host observer callback support,
  and active keyboard/voice availability registry queries. Hosts must not route
  input to engines that are not `TYPIO_ENGINE_READY`.

### Removed

- **Voice-only readiness was removed from `TypioVoiceEngineOps`.**
  `TypioVoiceEngineOps::is_ready` and the internal `VoiceEngine::is_ready`
  path are replaced by base engine availability.

## [0.1.4] - 2026-06-02

### Changed

- **ADR-0013: amended host-managed selection flags.** Wrote ADR-0013
  superseding ADR-0012: split `COMMIT` into `COMMIT` (Space) and
  `COMMIT_RAW` (Enter), extended `INDEX_PICK` to digit keys 0–9, fixed
  `TYPIO_HOST_SEL_ALL = 0xF`. Updated C header comment, Rust ABI doc
  comments, engine.h Doxygen, and all cross-references across docs.

## [0.1.3] - 2026-06-02

### Fixed

- **`CPluginAdapter` missing `commit_candidate` implementation.** The Rust
  adapter for C keyboard engine plugins (`src/c_api/plugin_adapter.rs`)
  implemented `process_key`, `get_active_mode`, and `set_active_mode`, but
  left `commit_candidate` as the trait default which returns
  `EngineError::NotSupported`. This caused host-managed candidate selection
  (ADR-0012) to silently fail for every C plugin engine: Space, Enter, and
  digit keys were consumed by the host but the commit callback never reached
  the engine. Added the missing `commit_candidate` dispatch to the C vtable.

## [0.1.2] - 2026-06-02

### Changed

- **Added `TYPIO_HOST_SEL_COMMIT_RAW` flag (ADR-0012).** Engines can now
  distinguish between Space (commit selected candidate) and Enter/KP_Enter
  (commit raw preedit text as-is). This lets compose-style engines support
  the common IME convention where Enter submits the untransformed input.

## [0.1.1] - 2026-06-02

### Changed

- **`host_managed_selection` redesigned as capability flags (ADR-0012).**
  Replaced the coarse `bool` with a `uint32_t` bit-mask of
  `TypioHostManagedSelection`:
  - `TYPIO_HOST_SEL_NONE` (0) — engine handles everything.
  - `TYPIO_HOST_SEL_NAVIGATE` — host intercepts Up/Down/Left/Right.
  - `TYPIO_HOST_SEL_COMMIT` — host intercepts Enter/Space.
  - `TYPIO_HOST_SEL_INDEX_PICK` — host intercepts 0–9.
  Engines can opt in to individual capabilities instead of accepting the
  entire host-managed UX contract. This resolves the input-domain overlap
  problem where digits or space are legitimate preedit characters (e.g.
  compose `^1` → `¹`).
- **ABI minor bumped 1 → 2.** The `TypioComposition` layout changed
  (`host_managed_selection` offset shifted from `bool` to `uint32_t`).

### Removed

- Backward-compatible `bool host_managed_selection` parsing. Hosts and
  engines must update to the new flag semantics in one go.

## [0.1.0] - 2026-06-02

### Added

- **`TypioKeyboardEngineMode` profile fields.** Mode struct now carries
  `profile_id`, `profile_label`, and `description` for engine-defined
  active profiles (e.g. Rime schema).
- **`list_modes` keyboard op.** Engines declare all supported modes;
  the host uses this for UI and cycling order.
- **`commit_candidate` keyboard op (ADR-0012).** Host-managed candidate
  selection: the host intercepts navigation/selection keys and calls back
  to the engine via `typio_input_context_commit_candidate`.
- **`typio_input_context_commit_candidate` C ABI bridge.** Dispatches
  to `EngineRegistry::commit_candidate_active_keyboard`.

### Removed

- **`TypioKeyboardEngagement` and all engagement-based routing.** The
  host no longer pre-filters keys based on engine-declared engagement.
  All keys go to the active engine; `process_key` return values decide
  routing. Engines that previously declared `OFF` or `PASSTHROUGH` now
  simply return `TYPIO_KEY_NOT_HANDLED` or rely on host deactivation.
- **Old mode surface:** `TypioEngineMode`, `TypioModeClass`,
  `get_mode`, `set_mode`, `notify_mode`, `TypioModeChangedCallback`.
  Replaced by `TypioKeyboardEngineMode`, `list_modes`, `get_active_mode`,
  `set_active_mode`, `notify_keyboard_mode`, `TypioKeyboardModeChangedCallback`.

### Changed

- **ADR-0011 Accepted:** Engine Mode as a first-class framework concept.
- **ADR-0012 Accepted:** Host-managed candidate selection.
- **ADR-0009 / ADR-0010 Superseded.**

## [0.0.5] - 2026-06-01

### Added

- **`base_keysym` field on `TypioKeyEvent`.** XKB returns effective
  keysyms (e.g. grave→asciitilde with Shift), but IME key bindings
  expect the unshifted base keysym. The new field carries the level-0
  keysym so engines can match key bindings correctly.
- **`typio_instance_set_engine_config_key` for engine-to-config write
  path.** Engines can now write their own config keys through libtypio
  instead of directly modifying files. The API validates keys against
  the registered schema, persists to disk, and notifies the engine via
  `on_config_change`.
- **Host-provided engine data/state directories.**
  `typio_instance_get_engine_data_dir` /
  `typio_instance_get_engine_state_dir` expose host-managed per-engine
  paths (HashMap-cached on `TypioInstance`).
- **Keyboard-domain status names and announcement salience (ADR-0010).**
  `TypioEngagement` → `TypioKeyboardEngagement`;
  `TypioEngineStatus` → `TypioKeyboardEngineStatus`; all
  status-reflection C symbols renamed to `*_keyboard_status*`.
  New `TypioStatusSalience` field (`QUIET`, `NOTABLE`) lets the engine
  set a ceiling on unprompted auto-reveal of status panels.
  ABI minor bumped 1 → 2.
- **`keyboard.disabled` / `voice.disabled` string config keys.** Engines
  listed here are excluded from activation.
- **`typio_registry_get_instance()`** added to C API for host-side
  config access.

### Fixed

- **`base_keysym` lost in Rust KeyEvent round-trip.** The Rust
  `KeyEvent` struct did not carry `base_keysym`, so engine plugins
  received `base_keysym=0` and could not match shifted key bindings.

### Changed

- **Voice auto-unload removed.** `IdlePolicy` variants,
  `UnloadScheduler`, idle thread, timerfd, and the
  `voice.unload_after_ms` config key are deleted. The
  `supports_unload()` engine backend method is also removed.

### Removed

- **Meson build files.** `meson.build` and `meson_options.txt` deleted;
  libtypio is cargo-only (`build.rs` emits the pkg-config file, C ABI
  headers ship from `include/`).

## [0.0.4] - 2026-05-31

### Fixed

- **Voice "Processing..." panel never cleared after recognition.** The dispatch
  path sent a `Result` event but omitted the follow-up `StateChange → Idle`
  event, so the host never hid the status panel. Now `fire_state_change(Idle)`
  fires after every `Result` event.

## [0.0.3] - 2026-05-31

### Added

- **Voice session start/stop.** `typio_instance_start_voice_session` and
  `typio_instance_stop_voice_session` now fully wire through the
  `EngineRegistry`, creating and tearing down a voice session on the
  active voice engine.

### Fixed

- **Double-free on voice result.** `VoiceEngine::process_audio` returned an
  owned `TypioVoiceResult`; the C adapter previously freed the `text` field
  and then dropped the Rust `CString`, causing a double-free. The adapter
  now takes ownership via `CString::from_raw` once.

## [0.0.2] - 2026-05-30

### Removed — legacy engine facade and compatibility shims

- **`typio_engine_activate` / `typio_engine_deactivate` removed.** Engine
  lifecycle is fully owned by `EngineRegistry`; these legacy C entry points
  are gone.
- **Internal dispatch helpers removed.** `_typio_engine_base_focus_in`,
  `_focus_out`, `_reset`, `_keyboard_process_key` deleted.
  `input_context/focus.rs` now routes directly through `EngineRegistry`.
- **`engine_has_voice` shim deleted.** The voice module no longer pretends
  to support a raw `TypioEngine *` path; all voice processing goes through
  `EngineRegistry::process_audio_active_voice`.
- **Backward compatibility removed.**
  - Old `[recent]` engine-state TOML section (replaced by `[keyboard]` /
    `[voice]` in 0.0.1) is no longer read.
  - Identity config key migration from the old flat key to `.engine` suffix
    removed.

### Fixed

- **`TypioFieldDefault` no longer implements `Copy`/`Clone`.** The union
  contains a raw pointer (`s`); implicit copying created a shallow-copy
  footgun. Added `unsafe fn raw_copy()` for callers that need a bitwise
  copy with explicit safety awareness.

### Changed

- **`KeyboardEngine::get_status` returns `Option<EngineStatus>`** instead of
  `Option<&EngineStatus>`. This lets the `CPluginAdapter` translate a C
  `TypioEngineStatus` into an owned Rust value without lifetime hacks.
  `CPluginAdapter::get_status` is now fully implemented (was `TODO`).
- **`InputContext::as_raw` takes `&self`** instead of `&mut self`; the raw
  pointer wrapper does not need mutable access.

## [0.0.1] - 2026-05-29

### Changed — composition fast-path for selection-only navigation

- **`typio_input_context_set_composition` now short-circuits when only `selected` (and `cursor_pos`) changed.** Candidate navigation (Up/Down) no longer re-allocates every `text`, `comment`, `label`, and preedit-segment `CString` when the content is identical to the previous composition. Added internal helpers:
  - `candidates_content_unchanged` — compares `count`, `page`, `total`, `has_prev`/`has_next`, and every candidate's `text`/`comment`/`label` via `CStr`.
  - `preedit_unchanged` — compares preedit segment count and each segment's `text`/`format`.
  - If both return true, only `selected` and `cursor_pos` are updated and the composition callback fires immediately.
- **Pre-allocated capacity hints in `set_composition`.** `preedit_segments.reserve(segs.len())` and `candidate_items.reserve(cands.len())` reduce Vec churn during the common case of replacing a composition with the same segment/candidate count.

### Changed — engine properties unified into the config schema layer (ADR-0008)

`TypioEngineSurfaceOps` no longer carries property accessors. Engine-owned
configuration is the config schema; the surface ops are imperative
commands only.

- **Removed from `TypioEngineSurfaceOps`:** `list_properties`,
  `get_property`, `set_property`.
- **Removed types:** `TypioEngineProperty`, `TypioEnginePropertyType`.
- **Removed C API:** `typio_engine_get_property`, `typio_engine_set_property`.
- **Added to `TypioEngineBaseOps`:** `on_config_change(engine, key, value)`
  — fires after the host commits a write to a key the engine owns; engines
  apply live side effects here (e.g. librime re-selecting a schema).
  Optional; engines that need only persistence leave it NULL.
- **Added to `typio/runtime/registry.h`:**
  - `typio_registry_invoke_command(registry, engine_name, id)` — invoke a
    command on a named engine. The pre-existing
    `typio_registry_invoke_active_keyboard_command` becomes a thin
    convenience over this.
  - `typio_registry_list_commands(registry, engine_name, *out_count)`
    paired with `typio_engine_command_list_free(commands, count)`.
  - `typio_registry_notify_config_change(registry, engine_name, key, value)`
    — host dispatch helper for engine notification after config writes.
- **Added to `typio/schema/config_schema.h`:**
  - `typio_config_schema_fields_with_prefix(prefix, *out_count)`
    paired with `typio_config_schema_fields_with_prefix_free(fields, count)`
    — efficient enumeration for `engines.<name>.*` introspection.

**Engine migration.** Engines that previously implemented property surface
ops move their declarations to the config schema layer:
- Export `typio_engine_get_config_schema` (or call
  `typio_config_schema_register_many` from `init`) with a
  `TypioConfigField[]` declaring each `engines.<name>.<key>`.
- Implement `on_config_change` to react to writes.
- Keep `list_commands` / `invoke_command` for imperative actions
  (`deploy`, etc.).

Engine ABI minor bumped; engines built against the previous header fail to
load with the existing `struct_size`-based diagnostic.

### Added — engine-owned config schema

- **Dynamic schema registration.** Engine plugins now declare their own
  `engines.<name>.*` configuration fields; libtypio no longer hardcodes
  engine-specific keys. New C ABI in `typio/schema/config_schema.h`:
  - `typio_config_schema_register(const TypioConfigField *)`
  - `typio_config_schema_register_many(const TypioConfigField *, size_t)`
  - `typio_config_schema_unregister(const char *)`
  Registered fields participate in `typio_config_schema_find`,
  `typio_config_schema_fields`, and `typio_config_apply_defaults` exactly
  like static entries. All caller strings are deep-copied; the caller may
  free its storage immediately after the call returns.
- **Optional plugin export `typio_engine_get_config_schema`.** Defined in
  `typio/abi/engine.h` as `TypioEngineConfigSchemaFunc`. The host loader
  calls it right after `typio_engine_get_info` and forwards the result to
  `typio_config_schema_register_many`, so engine config keys become visible
  to UI and defaulting before the engine is instantiated.

### Changed — config file split per process boundary

- **`typio.toml` renamed to `core.toml`.** libtypio's persistent config file
  moves from `$XDG_CONFIG_HOME/typio/typio.toml` to
  `$XDG_CONFIG_HOME/typio/core.toml`. The file now holds only framework-
  owned settings (engine selection, keyboard policy, notifications,
  shortcuts, voice runtime). Hosts that initialise `TypioInstanceConfig`
  with their own `config_dir` are unaffected; users on the default path
  must `mv typio.toml core.toml` once.
- **`display.*` removed from the libtypio schema.** Popup theme, candidate
  layout, font, mode indicator, and the `display.colors.*` overrides are
  no longer part of `core.toml`. They moved to `typio-wayland`'s own
  `wayland.toml` (see that repository's CHANGELOG for the migration path).
  Removed keys: `display.popup_theme`, `display.candidate_layout`,
  `display.font_size`, `display.font_family`, `display.popup_mode_indicator`,
  `display.colors.light.*`, `display.colors.dark.*`.

### Changed — config-key renames

Three config keys renamed for symmetry and clarity. There is no
deprecation alias; downstream `typio.toml` files, code that reads these
keys, and hosts that initialise `TypioInstanceConfig` must update in one
go.

| Before | After | Why |
|--------|-------|-----|
| `default_engine` (TOML) and `TypioInstanceConfig::default_engine` (C struct field) | `default_keyboard_engine` | Symmetry with `default_voice_engine`; removes the implicit "default of what?" |
| `shortcuts.switch_engine` (TOML) and `"switch_engine"` action ID | `shortcuts.switch_keyboard_engine` / `"switch_keyboard_engine"` | Same — the shortcut only ever switched the keyboard engine |
| `shortcuts.emergency_exit` (TOML) and `"emergency_exit"` action ID | `shortcuts.exit` / `"exit"` | Drops the "emergency" framing — it is the normal exit/disable shortcut |

The UI labels for the renamed shortcuts also follow ("Switch keyboard
engine" and "Exit"). Default bindings are unchanged.

### Removed — hardcoded engine schema entries

- Every `engines.<name>.*` field has been removed from libtypio's static
  base — including the `engines.basic.*` keys, since `basic` ships as the
  `typio-engine-basic` plugin. Affected keys (now owned by their respective
  plugins):
  - `engines.basic.printable_key_mode`, `engines.basic.compose`
    → `typio-engine-basic`
  - `engines.rime.shared_data_dir`, `engines.rime.user_data_dir`
    → `typio-engine-rime`
  Each plugin registers its own keys via the schema API above.

### Changed — debt elimination pass

This pass removes every remaining piece of compatibility shimming. There
is no deprecation grace period; downstream repos (`typiod-wayland`,
`typio-engine-*`, `typio-settings`, `typioctl`) must update against the
new headers in one go.

- **`TypioEngineManager` removed.** `TypioRegistry` (see
  [ADR-0005](docs/adr/0005-internal-engine-backend-abstraction.md)) is now the sole engine-management C surface. All
  `typio_engine_manager_*` symbols and `include/typio/runtime/engine_manager.h`
  are gone. The accessor on the instance is now
  `typio_instance_get_registry`.
- **`TypioPluginLoaderFunc` signature changed.** It now takes
  `TypioRegistry *` instead of `TypioEngineManager *`. Implementations
  call `typio_registry_register_plugin_keyboard` / `_register_plugin_voice`.
- **Legacy `typio_log_*` symbols removed.** Only the `typio_logger_*`
  family and the `typio_log_emit` / `typio_log_<level>` macros remain.
- **Header re-layering.**
  - `typio/abi/voice.h` now declares only the engine-side voice types
    (audio source ops, state enum, session event types). The host-only
    `TypioVoiceSession` lifecycle moved to **`typio/runtime/voice.h`**.
  - `typio/runtime/lifecycle.h` deleted. It described Wayland's
    keyboard-grab state machine; that logic now lives in
    `typiod-wayland`.
  - `typio/runtime/rime_schema_list.h` deleted. Rime schema discovery is
    accessed through the engine's generic `TypioEngineSurfaceOps`
    properties — works for any engine, not just Rime.
  - `typio/abi/renderer.h` deleted (orphan; renderer types belong to the
    frontend).
  - `typio/abi/engine_label.h` deleted; the trivial fallback is now done
    inline by callers.
- **Memory ownership normalized.** Every libtypio-allocated value is
  released through the `typio_free_*` family:
  - `char *` → `typio_free_string`
  - `char **` returned by `typio_registry_list_*` → `typio_free_string_array(list, count)`
  - `TypioEngineInfo *` → `typio_engine_info_free`
  - `TypioConfig *` → `typio_config_free`
  All internal `libc::malloc` / `libc::free` for string allocation are
  replaced with `CString::into_raw` / `CString::from_raw`, so libtypio
  always uses its own allocator. Documents in `typio/abi/string.h`.
- **`struct_size` discipline** added to caller-allocated structs
  (`TypioEngineInfo`, `TypioKeyEvent`; `TypioComposition` already had
  it). Engines/hosts must initialise the field; this is the additive-
  evolution mechanism within a major.
- **`TypioConfigValue` removed from the public ABI.** The discriminated
  union was an implementation detail and is no longer exposed.
  `typio_config_get` (the generic getter) is gone; use the typed
  `typio_config_get_{string,int,bool,float}` accessors.
- **`TypioCandidateList` removed.** Candidates are delivered as part of
  `TypioComposition` via the composition callback (see
  [ADR-0006](docs/adr/0006-composition-state-and-commit-event.md));
  `typio_input_context_get_candidates` is gone.
- **`TypioShortcutConfig` removed.** Shortcuts are now keyed by **action
  ID** strings:
  - `typio_shortcut_get(config, "switch_engine", &out)`
  - `typio_shortcut_default(action_id, &out)`
  Adding a new shortcut is additive (a new ID) rather than a struct
  layout break.
- **`TYPIO_*_ENGINE_DEFINE` macros hardened.** The expansion now emits
  every exported symbol with `extern "C"` linkage and `TYPIO_EXPORT`
  (default visibility on ELF, `__declspec(dllexport)` on Windows). Build
  engines with `-fvisibility=hidden -fvisibility-inlines-hidden`.
- **`libtypio.so` symbol surface restricted.** A linker version script
  hides every non-`typio_*` symbol — internal Rust runtime helpers no
  longer leak into the ABI.

### Changed — CLI and control-panel renames

- **CLI renamed `typio` → `typioctl`** (binary and repository). All
  invocations move from `typio status` / `typio engine ...` to
  `typioctl status` / `typioctl engine ...`. The full rationale lives in
  the `typioctl` repository's ADR set.
- **Control panel renamed `typio-control` → `typio-settings`**
  (repository, binary, GTK app id, package). The new name matches the
  GNOME/KDE control-centre vocabulary and clears the collision with the
  broader "control surfaces" concept. The full rationale lives in the
  `typio-settings` repository's ADR set.
- **No aliases.** Pre-1.0; downstream scripts and desktop files must
  switch in one go.

### Changed — documentation reference reorganisation

- **`docs/reference/api/` split by audience.** The single mixed folder is
  replaced by two folders that match the contract each audience consumes:
  - `docs/reference/host-abi/` — the **library ABI** of `libtypio.so`
    (instance, registry, input-context, config, event, log).
  - `docs/reference/plugin-abi/` — the **plugin ABI** that engine `.so`
    files implement (entry points, types, ops). The page formerly
    `plugin-abi.md` is now `plugin-abi/entry.md`; `engine-types.md` →
    `plugin-abi/types.md`; `engine-ops.md` → `plugin-abi/ops.md`.

  The `engine` vs `plugin` vocabulary distinction recorded in
  [ADR-0003](docs/adr/0003-plugin-engine-abi-dual-category.md) is
  preserved: an *engine* is any `TypioEngine` implementation (including
  the built-in `basic`); a *plugin* is an engine packaged as a loadable
  `.so` and is the only kind covered by `plugin-abi/`.

- **Reference pages filled to match their headers.** Each page now mirrors
  the full public surface of its corresponding `typio/abi/*.h` (and
  `typio/runtime/instance.h` for the host portion of the instance page),
  with explicit ownership/lifetime tables and `TypioResult` error rows
  rather than signature-only listings.
  - `host-abi/event.md` — added `TypioKeyEvent` struct (with `struct_size`
    and ownership), full predicate + key-class helper sets, `TypioModifier`,
    `TYPIO_KEY_*` constants, `TypioVoiceEvent` construction helpers, and
    the generic `TypioEvent` envelope.
  - `host-abi/config.md` — added `TypioConfigType`, `set_string_array`,
    `get_section`/`set_section`, array and key enumeration accessors,
    `remove`, `merge`, key-path syntax, default-value and wrong-type
    semantics, borrowed-string lifetimes, and corrected the
    `typio_config_to_string` ownership note (frees with
    `typio_free_string`, not libc `free`).
  - `host-abi/input-context.md` — added `TypioPreeditFormat`,
    `TypioPreeditSegment`, `TypioPreedit`, `TypioCandidate`,
    `TypioComposition` (with `struct_size`, `content_signature`,
    `revision`, pagination fields, and Unicode-scalar-counted offsets),
    `TypioContextCapability`, `TypioKeyProcessResult`, lifecycle
    (`new`/`free`), `set_capabilities`/`get_capabilities`,
    `set_user_data`/`get_user_data`, and borrowed-pointer lifetime
    rules.
  - `host-abi/instance.md` — added the engine-facing `typio/abi/instance.h`
    surface (`get_focused_context`, directory accessors, `get_config` /
    `get_engine_config`, `reload_config` / `save_config`, status icon
    notifications, sub-mode notifications), the voice-session slot
    accessors, and the per-application identity persistence family.
- **`host-abi/types.md` added.** Single page documenting `TypioResult`
  variants, the opaque handle catalogue (`TypioInstance`,
  `TypioRegistry`, `TypioInputContext`, `TypioConfig`,
  `TypioVoiceSession`), and the callback typedef family
  (`TypioCommitCallback`, `TypioCompositionCallback`,
  `TypioEngineChangedCallback`, `TypioVoiceEngineChangedCallback`,
  `TypioStatusIconChangedCallback`, `TypioModeChangedCallback`) with
  per-callback pointer-lifetime notes. Other host-abi pages link here
  instead of redefining these in place.

- **Bookmarks under `docs/reference/api/` will 404.** Update any
  external links to the new `host-abi/` or `plugin-abi/` paths.

### Added

- `typio_registry_get_engine_info` returns a fresh `TypioEngineInfo *`;
  release with `typio_engine_info_free`.
- `typio_registry_list_ordered_keyboards` honours the `engine_order`
  config key.
- `typio_free_string_array(list, count)` — canonical name-list deallocator.
- `typio_shortcut_get` / `typio_shortcut_default` — action-keyed lookup.
- `TYPIO_EXPORT` macro in `typio/abi/types.h`.

### Removed

- `typio_engine_manager_*` (entire family)
- `typio_log_set_level`, `typio_log_get_level`, `typio_log_set_callback`,
  `typio_log_set_recent_dump_path`, `typio_log_dump_recent`,
  `typio_log_dump_recent_to_configured_path`, `_typio_log`
- `typio_engine_label_from_info`
- `typio_input_context_get_candidates`, `TypioCandidateList`
- `typio_config_get` (generic getter), `TypioConfigValue` union
- `typio_engine_manager_free_engine_list` (use `typio_free_string_array`)
- `typio/runtime/lifecycle.h`, `typio/runtime/rime_schema_list.h`,
  `typio/abi/renderer.h`, `typio/abi/engine_label.h`,
  `typio/runtime/engine_manager.h`
- `typio_instance_get_engine_manager` (replaced by `_get_registry`)

### Migration

For hosts:

1. Replace every `typio_engine_manager_*` call with the matching
   `typio_registry_*` symbol.
2. Replace `typio_instance_get_engine_manager` with
   `typio_instance_get_registry`.
3. Update your `TypioPluginLoaderFunc` to take `TypioRegistry *`.
4. Replace every `libc::free` / `free()` of a libtypio return with
   `typio_free_string` (or `typio_free_string_array` for name lists,
   `typio_engine_info_free` for `TypioEngineInfo *`, `typio_config_free`
   for configs).
5. Drop `typio_log_set_*`; use `typio_logger_set_*` instead.
6. Replace `TypioShortcutConfig` field access with
   `typio_shortcut_get(config, "switch_engine"|"emergency_exit"|"voice_ptt", &binding)`.

For engines:

1. Initialise `TypioEngineInfo::struct_size = sizeof(TypioEngineInfo)`.
2. Initialise `TypioKeyEvent::struct_size = sizeof(TypioKeyEvent)` if you
   construct events on the stack to test plugin code.
3. Build with `-fvisibility=hidden`; the `TYPIO_*_ENGINE_DEFINE` macros
   now export the entry points correctly.

## Earlier in [Unreleased]

### Changed

- **Engine ABI rewritten — pre-1.0 reset.**
  All engine plugins must rebuild against the new headers.  See
  [`docs/dev/abi-stability.md`](docs/dev/abi-stability.md) for the new
  versioning + negotiation contract.
  - `TYPIO_ENGINE_ABI_MAJOR` reset to `0`, `TYPIO_ENGINE_ABI_MINOR = 1`.
  - **ABI version is now an out-of-band export.** Every engine MUST export
    `const TypioAbiVersion *typio_engine_abi_version(void)`.  The host
    rejects plugins whose major mismatches or whose minor exceeds the
    host's.  The `TYPIO_KEYBOARD_ENGINE_DEFINE` /
    `TYPIO_VOICE_ENGINE_DEFINE` macros emit this automatically.
  - **Capability bitfield replaced with named-string negotiation.**
    `TypioEngineCapability` enum and `TypioEngineInfo.capabilities` (u32)
    are removed.  Engines now declare `required_capabilities` and
    `optional_capabilities` as NULL-terminated `const char *const *`
    arrays.  Host advertises a supported set and rejects engines whose
    required set is not a subset.
  - **Cross-process protocol updated.** Both the UDS JSON status surface
    and the D-Bus engine-info dict emit `required_capabilities` /
    `optional_capabilities` as string arrays (was: `capabilities` uint32).
- **Build layout consolidated.** `core/` and `engines/` directories
  removed from libtypio; canonical source tree is the top-level
  `src/` + `include/` + `Cargo.toml`.  The remaining `meson.build` is a
  thin cargo wrapper for downstream meson subproject consumers.

### Added

- **`typio-abi-conformance` CLI.**  Validates engine plugins against the
  current ABI without involving the full host.  Checks ABI version
  export, info struct shape, factory signature, and capability arrays.
- **`docs/dev/abi-stability.md`.**  Single source of truth for the ABI
  versioning policy, negotiation algorithm, and the pre-1.0 disclaimer.

### Removed

- `TYPIO_ENGINE_ABI_VERSION` packed-int macro and the associated
  `TYPIO_MAKE_ABI_VERSION` / `TYPIO_ABI_VERSION_MAJOR` / `_MINOR`
  helpers.  Use the `TypioAbiVersion` struct exported by the plugin
  instead.
- `TYPIO_ENGINE_INFO_SIZE` and `TypioEngineInfo.struct_size` —
  the ABI version export is now the sole compatibility witness.
- `TypioEngineInfo.api_version`.
- `typio_engine_get_capabilities` (u32 bitfield).
- `TypioEngineCapability` enum and the `TYPIO_CAP_*` constants.
- `typio_engine_has_capability` no longer takes a `TypioEngineCapability`;
  it now takes a `const char *` capability name.

### Migration

For an existing engine targeting the previous ABI:

1. Remove `.api_version`, `.struct_size`, and `.capabilities` fields
   from your `TypioEngineInfo` static.
2. Add `.required_capabilities` and `.optional_capabilities` arrays of
   capability name strings (NULL or NULL-terminated).
3. Either use the `TYPIO_KEYBOARD_ENGINE_DEFINE` /
   `TYPIO_VOICE_ENGINE_DEFINE` macros (which emit the ABI version
   export), or define `typio_engine_abi_version()` by hand.
4. Run `typio-abi-conformance <your-plugin.so>` to verify.

### Changed (logging — see #log subsection below)

- **Logging system completely rewritten with full Inversion of Control.**
  The library no longer defaults to `stderr`; without a host callback, log
  records are silently retained in an internal ring buffer only.
  - New structured callback `TypioLogCallback` receives `TypioLogEvent`
    (`level`, `message`, `domain`, `file`, `line`, `timestamp_ms`).
  - New C API: `typio_logger_init`, `typio_logger_set_callback`,
    `typio_logger_set_level`, `typio_logger_get_level`,
    `typio_logger_set_recent_capacity`, `typio_logger_dump_recent`,
    `typio_logger_shutdown`, `typio_log_emit`.
  - Added `TYPIO_LOG_TRACE` level; levels now ordered `Trace < Debug < Info <
    Warning < Error`.
  - Rust internals use the standard `log` crate (`log::info!`, `log::warn!`, …).

### Removed

- **All legacy logging symbols:** `_typio_log`, `typio_log_set_callback`,
  `typio_log_set_level`, `typio_log_get_level`,
  `typio_log_set_recent_dump_path`, `typio_log_dump_recent_to_configured_path`,
  and the internal `log_msg` helper are gone.
- **`TypioInstanceConfig.log_callback` and `log_user_data` fields.** Logging
  configuration is now decoupled from instance lifecycle; hosts call
  `typio_logger_set_callback` independently.
