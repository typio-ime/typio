# typio-engine-check

Black-box conformance testing and validation for manifest-declared Typio engine processes.

`typio-engine-check` reads the same `typio-engine-*.toml` file as the daemon, launches
the declared executable with a private Engine Protocol channel on fd 3, and
checks the production boundary. It never loads engine code into the check process.

| Dimension | Checks |
|---|---|
| **Protocol** | required manifest fields, protocol/type, spawn, EngineHello, identity, schema namespace, HostHello |
| **Behavior** | initialization, availability, keyboard or voice request, clean shutdown |
| **Resource** | freedesktop icon name, asset presence, and placement |

Only `FAIL` makes the command exit non-zero. A `WARN` identifies legal but
suspicious behavior, such as a keyboard declining a plain printable key.

## Usage

```bash
cargo run -p typio-engine-check --bin typio-engine-check -- \
  ../typio-engines/typio-engine-compose/typio-engine-compose.toml

typio-engine-check <typio-engine-*.toml> [options]

    --package <dir>    Override the package root for resource checks
    --only <dims>      Comma-separated: protocol,behavior,resource
    --check <name>     Run/report one named check
    --list             List dimensions
    --help, -h         Show help
```

The repository also includes a minimal worker fixture for protocol work:

```bash
cargo build -p typio-engine-protocol --example hello_worker
typio-engine-check crates/typio-engine-protocol/examples/typio-engine-hello.toml
```

The tool shares `typio-engine-manifest` and `typio-engine-protocol` with the
daemon, including frame limits and typed payload decoding. A worker that passes
check is therefore exercised through the same handshake and request forms used in
production; engine-specific correctness still belongs in the engine's own test
suite. Check supplies isolated temporary config, data, and state roots and removes
them after the worker exits.
