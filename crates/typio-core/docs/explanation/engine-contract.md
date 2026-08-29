# Engine contract

An engine transforms host inputs into typed outputs while remaining ignorant
of Wayland, panel rendering, TIP, and daemon scheduling.

## Lifecycle

1. The worker sends cheap metadata and schema in `EngineHello`.
2. A discovery probe may end there. The worker exits cleanly when the channel
   closes.
3. For activation, `HostHello` confirms identity and supplies the exact config,
   data, and state roots. `Initialize` starts heavy dictionaries, models, or
   language services.
4. Focus, reset, key, audio, mode, command, availability, and reload operations
   are request/reply transactions.
5. `Deactivate` drops optional transient resources; `Shutdown` exits the
   process.

Each response echoes its request id. A transport or decode error poisons the
worker; the runtime discards it and starts a fresh process. The respawn runs
asynchronously on a detached thread and is installed by the next engine use;
while it is in flight the backend reports no engine, so callers degrade
gracefully (keyboard keys pass through to the application) instead of
blocking the caller on a multi-second spawn.

## Ownership

| Engine owns | Runtime and host own |
|---|---|
| Language or speech implementation | Wayland and application protocols |
| Dictionaries, models, learning data | Focus and input-method session policy |
| Per-context language state keyed by context id | Context ids and key normalization |
| Mode definitions and current mode | Mode caching and user-visible switching |
| Composition, commit, transcription results | Applying output to compositor/UI |
| Engine command implementation | TIP exposure and authorization |

No memory is shared. All strings, candidate lists, schema fields, audio bytes,
and mode records are copied into bounded frames and decoded into owned values.

## Keyboard transaction

`ProcessKey` contains the context id, key state, hardware code, keysym,
modifier mask, Unicode scalar, timestamp, repeat marker, and base keysym. Its
reply contains exactly one routing result plus any ordered output records:

- `NOT_HANDLED`: forward the key to the client;
- `HANDLED`: consume it;
- `PASS_THROUGH`: preserve the current composition but forward the key;
- `COMPOSITION`: replace the complete preedit/candidate snapshot;
- `COMMIT`: enqueue finalized text;
- `CLEAR`: replace composition with the empty state;
- `ACTIVE_MODE`: update cached mode after the causative request.

Composition is state; commits are ordered events. One request may commit text
and leave a new composition, so they are intentionally separate channels.

## Voice transaction

The host captures bounded little-endian `f32` mono samples and sends them in
`ProcessAudio`. A voice engine may return `TEXT` or no text for silence. Audio
capture remains a host concern; model loading and transcription remain engine
concerns.

## Configuration and commands

Schema is metadata published before initialization. Keys must live below
`engines.<name>.`, allowing settings and strict validation while the engine is
inactive. The daemon owns persistence in `core.toml`; a reload request tells an
active worker to refresh its namespaced settings.

Persisted values use schema fields. One-shot actions use `COMMAND` records and
`InvokeCommand`. Engines must not create private TIP methods or overload config
values as command triggers.

The canonical codec and runnable example are in
[`typio-engine-protocol`](../../../typio-engine-protocol/README.md).
