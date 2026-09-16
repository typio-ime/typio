# Engine contract

An engine transforms host inputs into typed outputs while remaining ignorant
of Wayland, panel rendering, TIP, and daemon scheduling.

## Lifecycle

1. The worker sends cheap metadata and schema in its opening handshake.
2. A discovery probe may end there. The worker exits cleanly when the channel
   closes.
3. For activation, the host's handshake reply confirms identity and supplies the
   exact config, data, and state roots. Initialization starts heavy
   dictionaries, models, or language services.
4. Focus, reset, key, audio, mode, command, availability, and reload operations
   are request/reply transactions.
5. A deactivation request drops optional transient resources; a shutdown
   request exits the process.

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

A key request carries the context id together with the complete normalized key
description: key state, hardware code, keysym, base keysym, modifier mask,
Unicode scalar, timestamp, and repeat marker. Its reply carries exactly one
routing result plus any ordered output records.

The routing result has three values:

- a **not-handled** result forwards the key to the client;
- a **handled** result consumes the key;
- a **pass-through** result is defined by the protocol to preserve the current
  composition while still forwarding the key.

The legacy composing and committed aliases decode to handled. The host does not
currently honour the pass-through intent: its key path treats every result other
than not-handled as consumption, so pass-through behaves exactly like handled
today. [Modifier-Key Consumption](modifier-key-consumption.md) records that
divergence in full.

Output records are a separate channel from the routing result: a composition
replaces the complete preedit and candidate snapshot, a commit enqueues
finalized text, a clear replaces the composition with the empty state, and an
active-mode record updates the cached mode after the causative request.

Composition is state; commits are ordered events. One request may commit text
and leave a new composition, so they are intentionally separate channels.

## Voice transaction

The host captures bounded little-endian 32-bit float mono samples and sends them
as an audio request. A voice engine may return recognized text or no text at all
for silence. Audio capture remains a host concern; model loading and
transcription remain engine concerns.

## Configuration and commands

Schema is metadata published before initialization. Keys must live below the
engine's own namespace, which allows settings and strict validation while the
engine is inactive. `typio-runtime` owns persistence; a reload request tells an
active worker to refresh its namespaced settings.

Persisted values use schema fields. One-shot actions use command records and an
invoke-command request. Engines must not create private TIP methods or overload
config values as command triggers.

The canonical wire vocabulary and a runnable example live in the
[Engine Protocol Reference](../reference/engine-protocol.md).
