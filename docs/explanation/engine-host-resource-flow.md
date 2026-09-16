# Engine-to-host resource flow

Typio engines are manifest-declared worker processes. No engine-owned pointer or
executable code crosses into the daemon. Each resource travels one typed
channel, and each channel has a single authority:

1. The host resolves and parses a manifest, which supplies the engine's
   identity, worker command line, declared languages, and capabilities.
2. The host starts the declared worker and the worker answers with its opening
   handshake, carrying cheap metadata and its configuration schema.
3. The process backend decodes replies into owned values — mode records,
   composition and commit output, recognized text, and availability — and hands
   them to the runtime registry.
4. The registry exposes snapshots and events to the host surfaces: the control
   service, the tray, and the Panel.
5. Host surfaces send typed requests back through the registry, and the
   registry writes them to the worker as bounded frames on the private protocol
   file descriptor.

| Resource | Authority | Validation and lifetime |
|---|---|---|
| Manifest metadata | package | shared parser; registry slot |
| Configuration schema | the worker's opening handshake | typed decode and namespace check; registry slot |
| Mode | worker reply | typed decode; cached until replaced |
| Composition and commit | worker reply | bounded frame; input context |
| Voice text | worker reply | hex and UTF-8 validation; one request |
| Availability | worker reply | closed enum; refreshed on query |
| Commands | worker reply | non-empty ids; one request |

The manifest and handshake identities must agree. Discovery validates schema
and then closes the probe without completing activation; activation repeats the
handshake and initializes the worker. A poisoned channel is never reused.

Choose an existing typed channel when adding data: stable display metadata
belongs in the manifest, persisted options in schema, current sub-mode in mode
records, text state in composition/commit records, readiness in availability,
and user-invoked actions in commands.
