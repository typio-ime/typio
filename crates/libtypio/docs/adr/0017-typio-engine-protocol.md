# ADR-0017: Typio Engine Protocol and engine-process registration

- **Status**: Accepted
- **Date**: 2026-06-08
- **Deciders**: Typio maintainers
- **Supersedes**: [ADR-0015](0015-ipc-only-engine-backend.md)
- **Amends**: [ADR-0016](0016-out-of-process-active-mode-reflection.md)

## Context

ADR-0015 correctly moved engine execution out of the host process, but its
terminology named the public contract after the transport mechanism: IPC. That
left three concepts mixed together:

- the product-level purpose, an engine process;
- the stable contract, Typio Engine Protocol;
- the implementation technique, a Unix file descriptor backed by a socketpair.

The line-oriented stdin/stdout worker protocol also made logs and protocol data
share the same streams. That is fragile for long-lived engines, especially
native engines whose dependencies may write diagnostics to standard output.

## Decision

libtypio exposes one host registration path:
`typio_registry_register_engine_process`. The registry stores
`EngineBackend::Process`. Public names use "engine process" for lifecycle and
"Typio Engine Protocol" for the contract. "IPC" is reserved for generic
implementation discussion or for unrelated host-control protocols.

Typio Engine Protocol version `1.0` runs on a private file descriptor passed to
the engine process as fd 3. The host sets `TYPIO_ENGINE_PROTOCOL=1.0` and
`TYPIO_ENGINE_FD=3`, connects the descriptor to a Unix socketpair, closes stdin
for protocol traffic, and leaves stdout/stderr for logs.

Every message is a bounded binary frame with magic `TYEP`, major/minor version,
message type, flags, request id, and payload length. The current payload keeps
the existing request/response line semantics inside the frame; the transport is
now versioned and can move to a typed payload schema later without returning to
stdio.

Engine manifests identify this contract with:

```toml
protocol = "typio-engine-protocol"
```

## Alternatives considered

- **Keep "IPC engine" as the public name**: Rejected because it describes a
  technique, not the caller's intent or the contract boundary.
- **Keep stdin/stdout framing**: Rejected because ordinary logs can corrupt the
  protocol and because stdio makes embedding third-party native dependencies
  harder to reason about.
- **Add a `v2` suffix**: Rejected because there is no need to encode version
  churn into stable product names. Versioning belongs in the protocol header and
  manifest contract.
- **Design the typed payload schema in the same change**: Deferred because the
  transport boundary is the urgent correctness fix. Keeping payload semantics
  stable limits the blast radius while still eliminating the stdio coupling.

## Consequences

- Positive: Public names now describe the engine lifecycle and protocol purpose.
- Positive: Engine logs cannot corrupt host/engine traffic.
- Positive: Protocol frames are versioned, bounded, and greppable by magic.
- Trade-off: Engine executables must learn fd 3 framing.
- Trade-off: The payload schema still needs a future cleanup to remove legacy
  line commands.
- Negative (accepted): Hosts and engines must rebuild against the renamed
  registration API and manifest protocol value.
