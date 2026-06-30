# ADR-0015: IPC-Only Engine Backend

- **Status**: Accepted
- **Date**: 2026-06-05
- **Deciders**: Typio maintainers

## Context

The backend abstraction allowed both in-process C plugin adapters and a future IPC backend. That split left fault isolation optional and forced hosts to understand which engine transport to choose. Engines process keystrokes and audio, so the long-term default must isolate untrusted or buggy engine code from the host process.

## Decision

libtypio exposes a single engine registration path: `typio_registry_register_ipc_engine`. The registry stores `EngineBackend::Ipc` only. The IPC backend starts the worker argv supplied by the host and speaks the line-oriented worker protocol for lifecycle, key processing, composition snapshots, candidate commit, mode queries, availability, and voice audio.

The C engine ABI remains available for engine implementation code, but libtypio no longer adapts C vtables into in-process Rust engines. Hosts that need to run a C ABI engine do so by starting a worker process that owns the dynamic library.

## Alternatives considered

- **Keep FFI and IPC backends**: Rejected because two active transports keep the risk model and engine contract ambiguous.
- **Let libtypio discover and load workers**: Rejected because discovery is platform policy; libtypio owns registration and dispatch.
- **Remove the C engine ABI immediately**: Rejected because existing engines can be isolated by a worker process without keeping daemon-side in-process loading.

## Consequences

- Positive: libtypio has one runtime engine transport.
- Positive: Hosts register engines without passing dynamic-library handles or close callbacks.
- Trade-off: Tests and hosts need a worker executable for engine behavior.
- Trade-off: Worker protocol compatibility becomes part of the host/core contract.
- Negative (accepted): `typio_registry_register_plugin_keyboard` and `typio_registry_register_plugin_voice` are removed.
