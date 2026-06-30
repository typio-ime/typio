# typioctl

Command-line client for the Typio input method framework. Installs as
`typioctl`.

Speaks the daemon's TIP v1 protocol — JSON-RPC 2.0 over a Unix Domain
Socket — to query and switch engines, read and write configuration, and
inspect status. Pure Rust with no link against libtypio; the only runtime
requirement is the daemon's socket.

## Building

```sh
cargo build --release -p typioctl
cargo install --path crates/typioctl
```

## Usage

Resource + verb. Engine name is always explicit.

```sh
typioctl engine list                       # all engines (* marks active)
typioctl engine show rime                  # properties + commands + values
typioctl engine use rime                   # set active
typioctl engine next                       # cycle to next keyboard
typioctl engine get rime schema            # read a property
typioctl engine set rime schema luna_pinyin
typioctl engine do  rime deploy            # invoke a command

typioctl config get  ui.theme              # arbitrary dotted keys
typioctl config set  ui.theme dark
typioctl config list --prefix engines.     # everything under engines.*
typioctl config show                       # dump (TOML)
typioctl config reload

typioctl daemon status
typioctl daemon stop
typioctl daemon version

typioctl -o json engine list               # machine-readable output
```

The daemon (`typio`) must be running. See
[`docs/reference/command.md`](docs/reference/command.md) for the full reference and
[ADR-0004](docs/adr/0004-resource-verb-cli-schema-on-tip.md) for the design
rationale.
