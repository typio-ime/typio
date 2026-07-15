# Config and runtime ownership

Typio separates persisted intent, live daemon state, client-side edits, and
temporary view state. A value belongs to one authority at a time; clients must
not substitute one domain for another.

## State domains

| Domain | Authority | Examples |
|---|---|---|
| Persisted config | Owner of the corresponding config file | Shortcuts, notification policy, engine options, popup theme |
| Persisted runtime choice | Engine state store | Last active language, keyboard engine, voice engine |
| Runtime state | Running daemon | Active engines, availability, focused app, current keyboard mode |
| Staged edit | One client process | A settings form not yet accepted by `config.set` |
| View state | Widget or CLI rendering | Expanded sections, selected row, transient error text |

The daemon owns runtime truth. Persisted values express user intent and may
temporarily differ while an engine starts, fails, or rolls back.

## File ownership

| File | Writer | Contents |
|---|---|---|
| `core.toml` | libtypio through the host | Framework policy and `engines.<name>.*` options |
| `platform.toml` | Reference host | Frontend presentation |
| `engine-state.toml` | Engine registry | Active language and last-used modality choices |
| `identity-engine-state.toml` | Identity state layer | Per-application engine and mode memory |

A control surface talks to the owning daemon through TIP. It does not read a
file to infer runtime state and does not write `core.toml` directly while the
daemon is running.

## Control-plane authority

TIP v3 is the reference host's owner-only Unix-socket JSON-RPC surface.

| Need | Read through | Write through |
|---|---|---|
| Config value or schema | `config.get`, `config.list` | `config.set`, `config.unset` |
| Active keyboard/voice engine | `daemon.status`, `engine.list` | `keyboard.use`, `voice.use` |
| Active language | `daemon.status`, `language.list` | `language.use` |
| Engine action | `engine.describe` | `engine.invoke` |
| Change notification | `events.subscribe` | Not applicable |

D-Bus is used where the desktop requires it, notably StatusNotifierItem and
login/session integration. It is not the authority for Typio config or engine
control.

## Binding rules

A UI control declares one read authority and one write authority:

- a config editor reads schema plus the current/default value and writes with
  `config.set`;
- an engine selector reads live status and writes with the modality-specific
  activation method;
- a command button reads command metadata from `engine.describe` and invokes
  the command by stable id;
- a mixed control may display live state while separately editing persisted
  policy, but must label those values rather than silently merging them.

Unknown runtime state remains unknown. A client must not select the first
dropdown item, infer activation from `core.toml`, or overwrite a newer staged
edit with an older reply.

## Schema and runtime state

`TypioConfigField` is authoritative for persisted fields. Its optional
`runtime_property` member is a host-defined logical state key, not a D-Bus
name. Most fields should leave it NULL; use it only when one stable runtime
value directly mirrors the persisted value.

Engine-owned fields arrive during the EngineHello discovery probe. This lets
clients render an inactive engine's settings without constructing its model or
dictionary. The host removes that schema when the engine registry slot is
unloaded.

## Apply and rollback

A config mutation follows this boundary:

1. parse and type-check the proposed value;
2. enforce schema choices and numeric bounds;
3. persist through the owning config API;
4. refresh affected runtime subsystems;
5. emit TIP change notifications.

If activation fails, the registry restores the previous usable engine when it
can. Keyboard and voice rollback are independent. Persisted intent is never
silently rewritten merely to hide a runtime failure.

## Regression requirements

New stateful features need tests for the paths they actually use:

- schema/default to client rendering;
- valid and invalid client writes;
- runtime status and notification to client rendering;
- delayed replies not overwriting newer edits;
- startup, reload, failure, and rollback;
- unload removing engine-owned schema and commands.

The test contract follows the authority. A runtime-driven selector is not
correctly tested by reading config, even if both happen to render as the same
kind of widget.

## Anti-patterns

- Reading config files directly from a control surface.
- Treating persisted intent as proof of current activation.
- Publishing the same mutable value as both an engine property and a config
  field.
- Inventing per-engine TIP methods when schema or `engine.invoke` is enough.
- Maintaining separate handwritten option lists in the daemon and settings
  client.
