# How to Install Typio

This guide assumes you have already [built Typio from source](../tutorials/01-getting-started.md).

## When to use this

Use this approach when you want a permanent system installation or are preparing a package. If you only want to run from the build tree, the tutorial is sufficient.

## Requirements

- Everything needed to build Typio (see [Getting Started](../tutorials/01-getting-started.md))
- A Wayland compositor that exposes `zwp_input_method_manager_v2`
- Applications using a working `zwp_text_input_manager_v3` path

## Build

```bash
cd libtypio
cargo build --release
```

## Installing engines

Engines are independent packages that produce engine executables and manifests.
Install workers under `<libexecdir>/typio/engines` and manifests under
`<datadir>/typio/engines`, where the host discovers them at runtime.

```bash
# Example: the compose engine
cd typio-engine-compose
cargo build --release
install -Dm755 target/release/typio-engine-compose \
  ~/.local/libexec/typio/engines/typio-engine-compose
```

Install a manifest whose `command` points at that absolute worker path, then
restart the host.

## Install

System-wide install of the core library and headers:

```bash
sudo install -Dm755 target/release/libtypio.so /usr/local/lib/libtypio.so
sudo cp -r include/typio /usr/local/include/
```

For a staging install (disposable, no root), copy the artifacts into a
local prefix:

```bash
mkdir -p ~/.local/lib ~/.local/include
cp target/release/libtypio.so ~/.local/lib/
cp -r include/typio ~/.local/include/
```

Installed paths:

- `/usr/local/lib/libtypio.so` — core shared library
- `/usr/local/include/typio/*.h` — C ABI headers

## Verification

```bash
typio version
typio --verbose
```

## Common issues

- **The host shows no external engines**: Check the manifest directory
  (`<datadir>/typio/engines`) and confirm each manifest's `command` points at
  an executable worker.
