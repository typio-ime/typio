# Configuration system

`typio-runtime` owns the persisted runtime configuration — engine registry,
shortcuts, notifications, voice, and per-engine settings — and the daemon reads
the host-consumed keys from it. The host adds its own Panel-styling settings in
a separate host-owned file, so appearance can be edited while the daemon is
stopped. The runtime parses its file into owned values, applies schema
defaults, and swaps a fully validated replacement on reload, so an invalid file
never partially mutates live state.

The schema has two sources:

| Source | Lifetime | Namespace |
|---|---|---|
| Built-in runtime fields | Process lifetime | framework keys |
| Engine-published schema records | Registered engine slot | one namespace per registered engine |

Discovery probes publish an engine's schema before heavy initialization. The
host validates the namespace and replaces all fields for one engine atomically.
Slot unload removes those fields, preventing stale settings from appearing as
live capabilities.

`typio-settings`, `typioctl`, and third-party clients use TIP's configuration
methods to inspect or change values. They do not edit runtime objects or talk
directly to workers. After a successful reload, active engines receive a reload
request; workers use the exact configuration root received in the activation
handshake and read only their own namespaced values.

Mutable settings and actions are intentionally distinct. Settings have typed
defaults and persistence. Actions are discovered and invoked through the engine
command surface.

See [Control Surfaces](control-surfaces.md) for the transport that carries
configuration requests, and the [Configuration Reference](../reference/configuration.md)
for the keys, their types, and their defaults.
