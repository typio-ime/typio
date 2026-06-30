# typio-core

The core library of the Typio input method framework — platform-neutral
business logic (config, input context, engine orchestration) implemented
in Rust and exposed through a hand-written C ABI.

This repository builds `libtypio.so` and `libtypio.a`, and installs the
public C ABI headers under `include/typio/`. Hosts and engine plugins link
against the library directly or include the headers.

`typio-core` is a pure framework crate: no engines are built in. The
`typio-engine-basic` fallback lives in the separate `typio-engine-basic`
repository (sibling to this one). All other engines, the platform host,
the CLI, and the control panel are separate repositories that consume
this one.

## Header layers

Public headers under `include/typio/` are partitioned by audience and
stability promise — see [docs/dev/contract-layers.md](docs/dev/contract-layers.md):

- `typio/abi/` — stable plugin ABI (engines include only `typio/abi/abi.h`)
- `typio/schema/` — user-facing config schema
- `typio/runtime/` — in-process embedding API for hosts

Rust engine authors do not need the C headers. They depend on the
**`typio-abi`** crate (a workspace member under `crates/typio-abi`)
which exports the same `#[repr(C)]` types, constants, and key symbols
used by the C headers.  This keeps Rust engines in sync with `typio-core`
without linking the full host library.

## Build

```bash
cargo build
cargo test
cargo build --release
```

Optional: to also build the basic engine plugin:

```bash
cd ../typio-engine-basic
cargo build --release
```

## Documentation

- [Full documentation](docs/index.md)
- [Architecture Overview](docs/explanation/architecture-overview.md)
- [Contract layers](docs/dev/contract-layers.md)
- [Contributing](CONTRIBUTING.md)

## License

See [LICENSE](LICENSE).
