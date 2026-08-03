# ADR-0046: Engine-Protocol-Only Runtime

- **Status**: Accepted
- **Date**: 2026-08-03
- **Supersedes**: ADR-0038's ABI workspace decision; framework ADR-0002,
  ADR-0003, ADR-0004's plugin-loading decision, ADR-0005's C wrapper, and
  ADR-0015's retained engine ABI

## Context

Typio had already moved every engine behind an executable boundary. The host
started a manifest-declared worker and exchanged framed messages over a private
file descriptor, but the worker implementations still used the old C plugin ABI
internally and `typio-core` still published a `libtypio.so`, C headers,
pkg-config metadata, and a separate `typio-abi` crate.

That left two contracts for one relationship. The process protocol determined
what could actually cross from an engine to the daemon, while the ABI imposed
layout stability, raw-pointer ownership, callback vtables, and dynamic-library
packaging that the daemon never consumed. `typio-vet` also tested the obsolete
in-process contract instead of the boundary used in production.

TIP is a third, unrelated contract: external clients use TIP over a filesystem
Unix socket. It must not be conflated with the private engine channel.

## Decision

Make Typio Engine Protocol the only engine boundary.

- An engine package installs a `typio-engine-*.toml` manifest and one worker
  executable. The host starts the worker with a private socket on fd 3. Engine
  code is never loaded into the daemon with `dlopen`.
- `typio-engine-protocol` owns typed frame, hello, request, reply, composition,
  mode, availability, and schema messages. `typio-engine-manifest` owns manifest
  parsing and path resolution. Host, core, and vet share these crates.
- `HostHello` carries the host-owned config, data, and state roots. Workers use
  those values instead of reconstructing command-line overrides from ambient
  environment variables.
- `typio-core` is an internal Rust `rlib`. Its instance, registry, configuration,
  input context, and voice session expose owned Rust values and safe borrowing;
  it no longer exports C symbols or a shared library.
- Remove `typio-abi`, all `typio/abi/*` and host-runtime C headers, the
  `libtypio.so` version script, ABI pkg-config metadata, vtables, factories,
  and ABI version negotiation.
- C and C++ engines may use source-level helpers inside their worker executable,
  but those helpers are implementation code, not a binary plugin contract.
  Compatibility with the daemon is negotiated only by Engine Protocol.
- `typio-vet` reads the production manifest, launches the executable in a
  separate process, validates EngineHello and schema ownership, then exercises
  lifecycle and modality requests over fd 3. It never loads engine code and
  gives every run isolated temporary runtime directories.
- Cross-thread host sources send typed daemon events. Registry and configuration
  mutation stays on the main loop; audio and inference use owned `Arc` handles.

## Alternatives Considered

- **Keep the ABI as an engine-internal convenience layer.** Rejected because it
  retains the layout and ownership obligations even though no production
  boundary needs them. Source helpers can provide convenience without claiming
  binary compatibility.
- **Keep a host C ABI for alternate embedders.** Rejected because there is no
  maintained C host. A speculative public surface would constrain the runtime
  while receiving no production or conformance coverage.
- **Fold engine traffic into TIP.** Rejected because the trust, lifecycle, and
  transport models differ. TIP serves user clients on a discoverable socket;
  an engine receives a private inherited channel and is supervised by the host.
- **Let each subsystem parse protocol text independently.** Rejected because it
  previously allowed host, vet, and workers to disagree on limits and required
  records. The typed protocol crate is the single Rust contract.

## Consequences

- Positive: there is one engine compatibility contract and one production-like
  conformance path.
- Positive: the daemon and framework no longer contain engine-supplied code,
  C callback user data, or ABI-driven cross-thread raw pointers.
- Positive: schema probing, normal activation, tests, and vet apply identical
  framing and validation rules.
- Positive: framework refactors are ordinary workspace changes rather than ABI
  migrations.
- Breaking: engines that require `libtypio.so` or publish only an ABI shared
  object must migrate to a self-contained Engine Protocol worker executable.
- Trade-off: non-Rust engine authors need a protocol adapter in their executable;
  the adapter is rebuilt with the engine rather than dynamically linked.
