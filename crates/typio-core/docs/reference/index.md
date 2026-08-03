# Core reference

- [Engine Protocol](engine-protocol.md) — frame layout, handshake, requests,
  and response records
- [`typio-engine-protocol` crate](../../../typio-engine-protocol/README.md) —
  canonical typed wire implementation and runnable worker example
- [Repository reference](../../../../docs/reference/) — manifests, TIP,
  configuration, CLI, and interface stability

There is intentionally no host ABI reference. `typio-core` is an internal
Rust workspace runtime, and engines communicate only through the process
protocol.
