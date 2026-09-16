# ADR-0008: Engine properties unified into the config schema layer

- **Status**: Accepted
- **Date**: 2026-05-29
- **Deciders**: Project maintainers

## Context

`libtypio` exposes two parallel mechanisms for the same concept — engine-owned configuration:

1. **`TypioEngineSurfaceOps`** (engine.h:283-294) — a runtime per-engine vtable: `list_properties`, `get_property`, `set_property`. Engines implement these to describe their typed knobs (e.g. the rime engine declares an enum property `schema` whose choices come from librime at runtime).
2. **Config schema layer** (schema/config_schema.h) — a registry of `TypioConfigField` entries with type, default, UI metadata, and `ui_options` choices. Static fields are hardcoded; engine-owned fields are registered dynamically via `typio_config_schema_register_many`. Persisted values live in the unified `TypioConfig` tree under dotted keys like `engines.rime.schema`.

The two mechanisms describe the same thing from two angles. Worse, today's engines use neither cleanly:

- The rime plugin implements `list_properties`/`get_property`/`set_property` *and* directly writes `engines.rime.schema` through `typio_config_set_string` (rime_control.c:106-111). It does **not** call `typio_config_schema_register_many`, so hosts can't introspect the property without instantiating the engine. The schema is described twice (in `list_properties` and implicitly by the surface ops contract) and registered nowhere formally — a test in `config_schema.rs:632-647` exists specifically to assert that `engines.rime.*` are not statically registered.
- The wayland host exposes `ActiveEngineProperties` and `ActiveEngineCommands` as IPC properties but the implementation is stubbed to `[]` (typio-wayland service.c:350-366) with a TODO that libtypio doesn't expose a host-callable wrapper to the active engine's surface ops. The stub has been in place because the right shape was unclear.

This duplication blocks the host-side cleanup described in ADR-0007 (control IPC owned by `typio-wayland`): without a single source of truth for engine properties, the host has to pick one of two parallel surfaces and contend with the other when an engine implements both. The CLI (`typioctl`) has accumulated engine-specific commands (`rime schema`, `rime deploy`) because the generic surface was unusable.

## Decision

Collapse engine properties into the config schema layer; keep engine **commands** (imperative actions) on the engine surface.

### Concrete API changes

1. **Remove** the property half of `TypioEngineSurfaceOps`:
   - `list_properties`
   - `get_property`
   - `set_property`

   `TypioEngineSurfaceOps` retains only `list_commands` and `invoke_command`. The `TypioEngineProperty` and `TypioEnginePropertyType` types are removed from the public ABI.

2. **Add** a notification callback to `TypioEngineBaseOps`:

   ```c
   /**
    * @brief A configuration key the engine owns has changed.
    *
    * Invoked by the host after the value is committed to the unified config
    * tree. Engines react with live side effects (e.g. librime reloading a
    * schema). Receives only keys under the engine's own `engines.<name>.*`
    * namespace plus any other key the engine has declared a watch on via
    * future API. Engines that need only persistence (no live reaction) leave
    * this NULL.
    */
   void (*on_config_change)(TypioEngine *engine, const char *key,
                            const char *value);
   ```

   This replaces the use of `reload_config` for property-driven side effects. `reload_config` remains for full-reload semantics triggered by external config file edits.

3. **Add** registry-mediated command surface so hosts can list and invoke commands on a named engine without holding a raw `TypioEngine *` (avoiding lifetime/ownership ambiguity for the host):

   ```c
   /* Borrowed array; valid until the next mutation of the named engine. */
   const TypioEngineCommand *typio_registry_list_commands(
       TypioRegistry *registry, const char *engine_name, size_t *out_count);

   TypioResult typio_registry_invoke_command(
       TypioRegistry *registry, const char *engine_name, const char *id);
   ```

   The existing `typio_registry_invoke_active_keyboard_command` becomes a thin convenience over the new by-name function.

4. **Add** a prefix lookup over the schema registry so hosts can enumerate an engine's properties in O(matching) instead of O(total):

   ```c
   const TypioConfigField *const *typio_config_schema_fields_with_prefix(
       const char *prefix, size_t *out_count);
   ```

### Migration

Every keyboard engine plugin (`typio-engine-rime`, `typio-engine-compose`, and any other in-tree consumer) updates in the same wave:

- Delete the engine's `list_properties`/`get_property`/`set_property` implementations.
- Call `typio_config_schema_register_many` in the engine's plugin entry to register the same properties as `TypioConfigField` entries (key, type, default, `ui_options` for enums, etc.). For rime's `schema` property the `ui_options` table is built at registration from librime's installed-schema list.
- Implement `on_config_change` for keys whose change must take live effect (rime: re-select schema on `engines.rime.schema`).

### Out of scope (deferred to later ADRs)

- The wire format of `typio-wayland`'s control IPC. Per ADR-0007, that contract lives in `typio-wayland`; this ADR only enables a clean implementation there.
- Unifying `TypioKeyboardEngine` and `TypioVoiceEngine` into a single type. The duplication exists but the user-facing cleanup does not require it.
- Watch registration on keys outside an engine's namespace.

## Alternatives considered

- **Keep both mechanisms; let engines pick.** Rejected: the duplication is exactly what made the rime plugin write through `typio_config_set_string` while also implementing `set_property` — three places the schema property partially lives. A "let engines pick" rule preserves the smell forever; a clean-sheet design picks one and removes the other.

- **Keep the surface ops; deprecate the config schema dynamic registration.** Rejected: the config schema layer already does more than the surface ops — UI metadata (`ui_label`, `ui_section`), defaults, runtime-property mirroring, key enumeration without engine instantiation. Surface ops require the engine to be loaded to discover its properties; the schema layer does not. Removing the more-capable mechanism is the wrong direction.

- **Add a host wrapper that proxies surface ops through to the schema layer, without removing either.** Rejected: a proxy that bridges two equivalent mechanisms is the textbook definition of code that nobody trusts. Either surface is authoritative; the other becomes a stale mirror. Pick one.

- **Defer: land the registry getters and prefix lookup only; leave surface ops in place.** Rejected per project mandate (greenfield rewrite, drop legacy debt). A "minimal" Phase A unblocks the IPC redesign but locks the duplication into every engine plugin shipping today; future cleanup then requires every plugin author to opt in. The cost of changing the ABI now (three engine plugins, all in-tree) is much smaller than the cost of changing it later.

## Consequences

- Positive: one source of truth for engine-owned configuration. Hosts read property metadata from `typio_config_schema_fields_with_prefix`, values from `typio_config_get_*`, write values via `typio_config_set_*` and `on_config_change`. The wayland host's `ActiveEngineProperties` stub becomes implementable in a few lines.

- Positive: engines shrink. `typio-engine-rime` loses ~80 lines of `rime_control.c` property plumbing and gains ~15 lines of schema registration and an `on_config_change` callback.

- Positive: clients can introspect engine capabilities without instantiating the engine — the schema is registered at plugin load, before `init`. The settings panel can render the rime configuration UI even when rime is not the active engine.

- Positive: `TypioEngineSurfaceOps` becomes single-purpose (commands only), which removes the awkward mismatch where engines that expose no commands still had to consider whether to expose properties through the surface.

- Trade-off: every keyboard engine plugin needs porting in the same change. In-tree this is three plugins. Out-of-tree (none today) would need a release-notes call-out and an engine ABI minor bump.

- Trade-off: `on_config_change` callbacks fire per-key. Engines that batch reloads (rime calling `RimeStart` on every property change) need to coalesce, or accept the redundant calls and rely on idempotence. The existing `reload_config` callback remains for full-config reloads where coalescing is the host's responsibility.

- Negative (accepted): the engine ABI breaks at the source level for any third-party engine. There are no third-party engines today. The ABI minor version bumps; engines built against the previous header fail to load with a clear `struct_size`-based diagnostic from the existing version check.

## Related

- [ADR-0002: C ABI as the only public interface](0002-c-abi-as-the-only-public-interface.md) — defines the surface this ADR modifies.
- [ADR-0005: Internal engine backend abstraction](0005-internal-engine-backend-abstraction.md) — `EngineBackend::Ffi` is the only consumer of `TypioEngineSurfaceOps`.
- [ADR-0007: IPC ownership — control surface to host](0007-ipc-ownership-host-and-engine-backend-deferred.md) — establishes that the wire protocol lives in `typio-wayland`; this ADR enables a clean implementation there.
