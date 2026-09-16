# Typio Engine Protocol

`typio-engine-protocol` is the only contract shared by the Typio daemon and
engine executables. It provides typed Rust messages plus the versioned frame
codec for the private fd 3 process channel. It contains no host runtime,
platform integration, dynamic-loader API, or C ABI.

## Process contract

The host starts one executable for each active engine. Before `exec`, it maps a
private full-duplex socket to fd 3 and sets:

```text
TYPIO_ENGINE_PROTOCOL=1.0
TYPIO_ENGINE_FD=3
```

Standard input is not a protocol channel. Standard output and standard error
are reserved for logs.

The lifecycle is:

1. The engine immediately sends `EngineHello` with request id 0.
2. During a discovery probe, the host validates the hello and may close the
   channel without replying. Heavy initialization must not happen yet.
3. During activation, the host sends `HostHello` with request id 0. It carries
   the exact host-owned config, data, and state roots, including command-line
   overrides; workers must not guess these paths from ambient XDG variables.
4. The host sends typed `Request` frames; the engine returns one `Reply` with
   the same request id.
5. A `Shutdown` request asks the worker to exit cleanly without a reply.

After malformed input, a request-id mismatch, or a broken channel, the stream
is discarded. Neither peer attempts byte-stream resynchronization.

## Frame header

All integer fields are network byte order.

| Offset | Size | Meaning |
|---:|---:|---|
| 0 | 4 | `TYEP` magic |
| 4 | 2 | major version (`1`) |
| 6 | 2 | minor version (`0`) |
| 8 | 4 | message type |
| 12 | 4 | reserved flags |
| 16 | 8 | request id |
| 24 | 4 | payload length |

Payloads are limited to 8 MiB. The public `Frame`, `EngineHello`, `HostHello`,
`Request`, `Reply`, and record types are the canonical encoding. Do not copy
Rust layout or use `repr(C)` across the process boundary.

## Rust engine

Depend on the workspace crate while developing beside Typio:

```toml
[dependencies]
typio-engine-protocol = { path = "../../typio/crates/typio-engine-protocol" }
```

Use `read_frame` and `write_frame` on fd 3, decode only the message type valid
for the current lifecycle state, and build replies from owned typed records.
See [`examples/hello_worker.rs`](examples/hello_worker.rs) for a complete
minimal keyboard worker.

Build and vet the example from the repository root:

```bash
cargo build -p typio-engine-protocol --example hello_worker
cargo run -p typio-engine-check -- \
  crates/typio-engine-protocol/examples/typio-engine-hello.toml
```

## Other languages

Non-Rust engines implement the same framed process protocol or compile a
source-level adapter into their worker executable. Such an adapter is an
implementation detail of that executable, not a versioned Typio binary ABI.
The installed engine package needs only its worker, manifest, and resources;
it never links `typio-runtime` or `libtypio.so`.

Run `typio-engine-check` against every package in CI. It starts the manifest-declared
worker and validates framing, identity, schema namespace, lifecycle,
availability, modality behavior, shutdown, and resources at the real process
boundary.
