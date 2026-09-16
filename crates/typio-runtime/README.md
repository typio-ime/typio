# typio-runtime

`typio-runtime` is Typio's platform-neutral Rust runtime. It owns configuration,
input contexts, engine registry policy, language and mode selection, and the
out-of-process engine backend.

The crate builds only an `rlib` and is embedded by `typio-daemon`. It publishes no
C headers or shared library, and it never loads engine code into the daemon.
Engine executables communicate through the typed
[`typio-engine-protocol`](../typio-engine-protocol/README.md) fd 3 contract.

```bash
cargo build -p typio-runtime
cargo test -p typio-runtime
```

See the [canonical Typio documentation](../../docs/index.md) and
[ADR-0046](../../docs/adr/0046-engine-protocol-only-runtime.md).
