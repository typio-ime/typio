# Getting Started: Your First Typio Build

By the end of this tutorial you will have:

- A local build of **libtypio** (the core library)
- A passing core test suite
- A build of the **typio** host that runs in your Wayland session
- Verified that the host starts and lists engines

> Typio keeps the Linux host, `libtypio`, `typio-abi`, `typio-vet`,
> and `typioctl` in one Cargo workspace. Engines live under the sibling
> `typio-engines/` checkout, and `typio-settings` remains a separate
> GTK application. See [project-layout.md](../dev/project-layout.md).

**Estimated time:** 15 minutes
**Difficulty:** Beginner

## Prerequisites

- Rust toolchain (latest stable `rustc` + `cargo`)
- A running Wayland session

### Default build dependencies

`libtypio` itself is pure Rust and has zero system dependencies:

| Component | Debian/Ubuntu | Arch Linux | Fedora |
|---|---|---|---|
| Rust toolchain | `rustup` (see [rustup.rs](https://rustup.rs)) | `rust` | `rust` |

The host (`typio`) and control tools need additional system
libraries (Wayland, D-Bus, etc.) — see the `typio` repo for its
own prerequisites.

## Step 1: Build libtypio

```bash
cd /path/to/projects/typio
cargo build --release -p typio-core
cargo test -p typio-core                 # core tests
```

> **libtypio tests are platform-free.** They do not need a Wayland session
> or D-Bus.

The library artifact appears at:

```
target/release/libtypio.so
```

C headers are under `include/typio/` for host and engine builds.

## Step 2: Build the compose engine (optional)

The `compose` Latin keyboard engine lives in a separate repo and is loaded
at runtime as an engine. It is optional — the framework runs with zero
engines installed (unhandled keys pass through unchanged).

```bash
cd /path/to/projects/typio-engines/typio-engine-compose
cargo build --release
```

The engine artifact appears at:

```
target/release/typio-engine-compose
```

## Step 3: Build and run the host

```bash
cd /path/to/projects/typio
cargo build --release -p typio-host --bin typio
```

The host binary appears at `target/release/typio`. Inside your Wayland
session:

```bash
./target/release/typio --engine-dir ../typio-engines/typio-engine-compose --verbose
```

You should see logs showing that the `compose` manifest was registered. Press
`Ctrl+C` to stop the host.

## Step 4: Add an engine (optional)

Engines are separate repositories that build engine executables and manifests.
The host discovers them at runtime from configured manifest directories:

```bash
cd /path/to/projects/typio-engines/typio-engine-rime
meson setup build
ninja -C build
```

Run the host with `--engine-dir /path/to/typio-engine-rime/build`; the build
manifest starts `./typio-engine-rime`. Packaged installations put workers under
`<libexecdir>/typio/engines` and manifests under
`<datadir>/typio/engines`.

## What's next?

- Want to install Typio permanently? See [How to install](../how-to/install.md)
- Want to configure Typio? See [How to configure](../how-to/configure.md)
- Want to understand the architecture? See [Architecture overview](../explanation/architecture-overview.md)
- Want to contribute code? See [Developer setup](../dev/setup.md)

## Troubleshooting

- **`Failed to connect to Wayland display`**: Make sure `WAYLAND_DISPLAY` is set and `XDG_SESSION_TYPE=wayland`.
- **`Session does not provide the Wayland input-method/text-input protocol stack`**: Your compositor must expose `zwp_input_method_manager_v2`. Verify with `wayland-info | grep zwp_input_method_manager_v2`.
