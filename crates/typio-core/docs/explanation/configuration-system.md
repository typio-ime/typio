# Configuration system

The daemon and `typio-core` own the single persisted `core.toml` configuration.
The runtime parses it into owned `ConfigValue` values, applies schema defaults,
and swaps a fully validated replacement on reload so invalid files never
partially mutate live state.

The schema has two sources:

| Source | Lifetime | Namespace |
|---|---|---|
| Built-in runtime fields | Process lifetime | framework keys |
| Engine `SCHEMA` records | Registered engine slot | `engines.<name>.*` |

Discovery probes publish engine schema before heavy initialization. The host
validates namespaces and replaces all fields for one engine atomically. Slot
unload removes those fields, preventing stale settings from appearing as live
capabilities.

`typio-settings`, `typioctl`, and third-party clients use TIP `config.*` methods
to inspect or change values. They do not edit runtime objects or talk directly
to workers. After a successful reload, active engines receive
`ReloadConfig`; workers use the exact config root received in `HostHello` and
read only their own namespaced values.

Mutable settings and actions are intentionally distinct. Settings have typed
defaults and persistence. Actions are discovered and invoked through the
engine command surface.
