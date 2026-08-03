# Engine author path

A Typio engine is a manifest-declared executable. The daemon starts it in an
isolated process and communicates over a private fd 3 channel. Engines are not
shared libraries and do not link the host runtime.

Use this order:

1. Read the [engine contract](explanation/engine-contract.md) for ownership and
   lifecycle.
2. Read the [`typio-engine-protocol` contract](../../typio-engine-protocol/README.md).
3. Copy the minimal
   [`hello_worker`](../../typio-engine-protocol/examples/hello_worker.rs) shape
   and replace its operation handling with the engine implementation.
4. Add a `typio-engine-<name>.toml` manifest. The manifest `name` and `type`
   must match `EngineHello`; `protocol` must be `typio-engine-protocol`.
5. Keep schema keys below `engines.<name>.` and publish them in `EngineHello`
   before heavyweight initialization.
6. Decode `HostHello` and use its exact config, data, and state roots. Do not
   guess host command-line overrides from XDG variables.
7. Run `typio-vet <manifest>` in the engine's test and packaging workflow.

Executable and manifest names use lowercase ASCII kebab case. The runtime name
is the suffix after `typio-engine-`; for example, `typio-engine-rime` has
`name = "rime"`. Engine repositories release independently from Typio, but
must pin a compatible Engine Protocol version while the contract is
experimental.

Rust engines should consume `typio-engine-protocol` directly. C and C++
engines may implement the wire format or compile an engine-local source adapter
into the executable. A source adapter must not become an installed shared
library, pkg-config dependency, or daemon-loadable ABI.
