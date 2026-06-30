# Developer Setup

This document is for contributors who will modify `typioctl` source code.

## Requirements

- Rust toolchain (latest stable `rustc` + `cargo`)

## Build

```bash
cargo build -p typioctl
cargo build --release -p typioctl
```

## Run

```bash
cargo run -p typioctl -- daemon status
cargo run -p typioctl -- engine list
cargo run -p typioctl -- help
```

The daemon must be running separately for most commands to work.

## Project layout

See [project-layout.md](project-layout.md) for a tour of the source tree.

## Submitting changes

See the [Pull Request Checklist](../../CONTRIBUTING.md#pull-request-checklist) in `CONTRIBUTING.md`.
