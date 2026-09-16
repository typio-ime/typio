# IPC Protocol Reference (TIP v3)

The Typio daemon exposes a **Unix Domain Socket** carrying length-prefixed JSON-RPC 2.0. This is the only control transport (ADR-0008).

## Socket location

| Variable | Default |
|---|---|
| Path | `$XDG_RUNTIME_DIR/typio/daemon.sock` (fallback: `~/.local/share/typio/daemon.sock`) |
| Mode | `0600`; peer uid must match the daemon uid (`SO_PEERCRED`). |

## Wire format

```text
[ 4 bytes: payload length in bytes (big-endian uint32) ]
[ N bytes: UTF-8 JSON payload                          ]
```

Used identically for requests, responses, and server→client event notifications. Max frame size: 1 MiB.

## JSON conventions

- Method names: dotted `namespace.action` (e.g. `engine.list`).
- All object keys and string values: **camelCase**.
- Numbers are JSON numbers; the `type` field of a config value distinguishes
  `string`, `int`, `bool`, `float`, and `array`.

## Request

```json
{ "jsonrpc": "2.0", "id": <int>, "method": "<dotted>", "params": <object> }
```

`params` is omitted when the method takes none.

## Response

Success:
```json
{ "jsonrpc": "2.0", "id": <int>, "result": <value> }
```

Error:
```json
{ "jsonrpc": "2.0", "id": <int>, "error": { "code": <int>, "message": "<str>" } }
```

Error codes follow the JSON-RPC 2.0 reserved range:

| Code | Meaning |
|---|---|
| `-32700` | Parse error |
| `-32600` | Invalid request |
| `-32601` | Method not found / not supported by target |
| `-32602` | Invalid params (unknown key/engine/etc.) |
| `-32603` | Internal error |

## Notification (server → client)

```json
{ "jsonrpc": "2.0", "method": "<topic>", "params": <payload> }
```

Notifications have no `id` and expect no reply. The client must have subscribed via `events.subscribe`; unsubscribed clients receive nothing.

## Methods

### `hello`

| Direction | params | result |
|---|---|---|
| C → S | `{}` | `{ protocolVersion, daemonVersion, capabilities: [string...] }` |

`protocolVersion` is an integer (`3` in this release; v2 replaced `engine.use` / `engine.next` with the modality-explicit `keyboard.*` / `voice.*` verbs — ADR-0026; v3 added the `language.*` namespace, `daemon.status.activeLanguage`, and the `language.changed` event — ADR-0031). `capabilities` enumerates the top-level namespaces the daemon supports — currently `["config", "engine", "keyboard", "voice", "language", "daemon", "events"]`.

### `config.*`

| Method | params | result |
|---|---|---|
| `config.get` | `{ key }` | `{ value, type, source }` (`source` is `"user"` or `"default"`) |
| `config.set` | `{ key, value }` | `{}` |
| `config.unset` | `{ key }` | `{}` |
| `config.list` | `{ prefix? }` | `[{ key, type, value, source, label, section, choices? }, ...]` |
| `config.show` | `{}` | `{ text, format: "toml" }` |
| `config.reload` | `{}` | `{}` |

`key` is a dotted path against the unified config tree. `value` is always a
string in `config.set`; the daemon strictly parses it using the schema or the
existing value's type. Boolean values accept `true`, `false`, `1`, or `0`.
Array values accept a JSON string array or a comma-separated string list.
Unknown keys and values outside schema choices or integer ranges are rejected.
`config.unset` removes the user value and restores the schema default when one
exists. `source` is `"user"` or `"default"`. For an engine-namespaced key
(`engines.<name>.<key>`) the daemon also delivers `on_config_change` to the
owning engine (archive ADR-0008).

### `engine.*` / `keyboard.*` / `voice.*`

The `engine.*` namespace is cross-modality and keyed by engine name (aggregate query + lifecycle). Activation and cycling are modality-explicit (ADR-0026): the keyboard and voice slots are orthogonal and simultaneously active, so each has its own verbs under `keyboard.*` / `voice.*`.

| Method | params | result |
|---|---|---|
| `engine.list` | `{}` | `[{ name, kind, displayName, active }, ...]` |
| `engine.describe` | `{ name }` | `{ name, kind, displayName, properties: [...], commands: [...] }` |
| `engine.invoke` | `{ name, command, args? }` | `{}` |
| `engine.load` | `{ path }` | `{ loaded, path }` |
| `engine.unload` | `{ name }` | `{ unloaded, name }` |
| `engine.reload` | `{ name, path? }` | `{ reloaded, name, path? }` |
| `keyboard.use` | `{ name }` | `{}` |
| `keyboard.next` | `{}` | `{ active }` |
| `keyboard.prev` | `{}` | `{ active }` |
| `voice.use` | `{ name }` | `{}` |
| `voice.next` | `{}` | `{ active }` |
| `voice.prev` | `{}` | `{ active }` |

`kind` (in `engine.list` / `engine.describe`) is `"keyboard"` or `"voice"`. `keyboard.use` / `voice.use` reject a `name` whose engine is not of the matching modality. Each property entry in `engine.describe` carries `{ key, type, value, label, choices? }`.

Engine commands are optional. `commands` is empty when a backend exposes no
command transport, and `engine.invoke` returns method-not-found (`-32601`) for
that backend.

`engine.load` loads a single engine manifest from an absolute `.toml` path.
`engine.unload` unregisters an engine by name, deactivating it first if active.
`engine.reload` combines unload and load: if `path` is provided, it must be an
absolute `.toml` path; if omitted, the daemon searches the configured
`engine_dirs` for `typio-engine-<name>.toml`. Reload fully parses and validates
the replacement manifest before unregistering the current engine, rejects a
manifest whose `name` differs, and reactivates an engine that was active before
the reload.

### `language.*`

| Method | params | result |
|---|---|---|
| `language.list` | `{}` | `{ languages: [{ tag, active }], active }` |
| `language.use` | `{ tag }` | `{}` |
| `language.next` | `{}` | `{ active }` |
| `language.prev` | `{}` | `{ active }` |

`tag` is a [BCP 47](https://www.rfc-editor.org/info/bcp47) language tag (see [Configuration § Language tag format](configuration.md)). The list is the enabled cycle: the `languages.enabled` config key when set, otherwise every engine-declared language in registration order. Activating a language retargets the keyboard and voice slots together (archive ADR-0018); the keyboard slot resolves in order — the `languages.<tag>.keyboard` config override (`"none"` forces an empty slot), then the engine last used for that language, then the first engine declaring it in registration order. `language.next` / `language.prev` return invalid-params when fewer than two languages are enabled or declared.

### `daemon.*`

| Method | params | result |
|---|---|---|
| `daemon.status` | `{}` | `{ version, protocolVersion, uptimeSeconds, activeKeyboardEngine, activeVoiceEngine, activeLanguage, runtime? }` |
| `daemon.version` | `{}` | `{ version }` |
| `daemon.stop` | `{}` | `{}` |

`runtime` is present when the runtime-state callback is wired (Wayland host); see `daemon.status` schema below.

### `events.subscribe`

| Method | params | result |
|---|---|---|
| `events.subscribe` | `{ topics?: [string...] }` | `{ subscribed: true }` |

Subscribes the calling connection to one or more topics. Omitting `topics` (or sending `[]`) subscribes to every topic. The subscription persists for the lifetime of the connection.

## Event topics

| Topic | Payload |
|---|---|
| `engine.changed` | `{ activeKeyboardEngine, activeVoiceEngine }` |
| `language.changed` | `{ activeLanguage, activeKeyboardEngine, activeVoiceEngine }` |
| `engine.statusChanged` | `{ modeId, modeLabel, displayLabel, iconName, profileId, profileLabel }` |
| `config.changed` | (reserved — emitted on config writes; payload TBD) |
| `runtime.changed` | (reserved — emitted on runtime-state edges) |
| `daemon.shuttingDown` | `{}` |

## `daemon.status` schema

| Field | Type |
|---|---|
| `version` | string |
| `protocolVersion` | int |
| `uptimeSeconds` | int |
| `activeKeyboardEngine` | string (empty if none) |
| `activeVoiceEngine` | string (empty if none) |
| `activeLanguage` | string (empty if none) |
| `runtime.frontendBackend` | string |
| `runtime.lifecyclePhase` | string |
| `runtime.virtualKeyboardState` | string |
| `runtime.keyboardGrabActive` | bool |
