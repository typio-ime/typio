# Developer Setup

This document is for contributors who modify Typio source code.

## Quick Start

All commands in this document run from the Typio repository root
unless a block says otherwise.

```bash
# one-time per optics checkout:
meson setup ../optics/build ../optics -Dtext=true --buildtype=debugoptimized
meson compile -C ../optics/build

# point flux-sys at the freshly built libflux (every shell that runs
# cargo build/test/run, or drop the two exports into ~/.bashrc / a local
# .envrc — the repo ships no committed copy):
export FLUX_BUILD_DIR="$PWD/../optics/build"
export FLUX_SOURCE_DIR="$PWD/../optics/libs/flux"

cargo build -p typio-host
cargo build -p typioctl
cargo build -p typio-settings
cargo test -p typio-host
./target/debug/typio --verbose
```

Typio links `libflux` straight out of the `optics/flux` Meson build
tree — there is no need to install flux system-wide, and no `LD_LIBRARY_PATH`
is required (flux-sys bakes an `-Wl,-rpath` to the build tree). `libtypio`,
`typio-abi`, `typio-vet`, `typio-client`, `typioctl`, and `typio-settings` are
Cargo workspace members in this repository, so framework, ABI, client, GUI,
CLI, and host changes build together.

The shipping daemon is the Rust `typio` binary from `crates/typio-host`.

## Prerequisites

Install these from your system package manager:

- Rust 1.85+ and Cargo
- Meson 1.0+, Ninja 1.10+, and `glslangValidator` to build `flux` from source
- C23 compiler and `pkg-config`
- Wayland client libraries and `xkbcommon`
- Vulkan headers (flux is Vulkan-rooted; the host uses only its CPU canvas, but
  the headers are required to build libflux)
- FreeType, HarfBuzz, fontconfig
- PipeWire's `pw-record` on the runtime PATH for voice capture

Versions are not capped; the project is tested against current Arch Linux
and Fedora releases.

## Repository Layout

The Typio product workspace lives beside the engine umbrella. The GPU/media
libraries live under a separate `optics/` umbrella because the canvas stack is
shared with non-Typio projects:

```text
projects/
├── typio/                        # host + framework + clients + settings GUI
├── typio-engines/
│   ├── typio-engine-compose/     # Cargo: Latin + compose-key keyboard
│   ├── typio-engine-rime/        # Meson: RIME-based Chinese keyboard
│   ├── typio-engine-mozc/        # Meson: Mozc-based Japanese keyboard
│   ├── typio-engine-sherpa/      # Meson: sherpa-onnx voice input
│   └── typio-engine-whisper/     # Meson: whisper.cpp voice input
└── optics/
    ├── libs/flux/                # C canvas library; build in-tree and point FLUX_BUILD_DIR at it
    ├── bindings/flux-rs/         # Rust bindings; published as git crate v0.1.0
    ├── libs/iris/, libs/lens/     # settings window and widget toolkit
    └── bindings/iris-rs/, bindings/lens-rs/
```

Typio resolves its framework, ABI, vet, command-line, and settings crates from
local workspace paths. Native optics libraries are sibling dependencies found
through pkg-config by their `-sys` crates.

## flux (C library)

`flux-sys` does not vendor the C source; its build script locates `libflux`
through pkg-config. The contributor workflow builds flux in its own tree and
points the build script at it, so no system install is needed.

**In-tree build (default).** Build flux once, then point `flux-sys` at the
build tree. `meson setup` is one-time per checkout; `meson compile` rebuilds
on demand:

```bash
meson setup ../optics/build ../optics -Dtext=true --buildtype=debugoptimized
meson compile -C ../optics/build

export FLUX_BUILD_DIR="$PWD/../optics/build"
export FLUX_SOURCE_DIR="$PWD/../optics/libs/flux"   # optional: bindgen from this checkout
```

`flux-sys` prepends the build tree's `meson-uninstalled/` to
`PKG_CONFIG_PATH` and bakes an `-Wl,-rpath` for it, so binaries find
`libflux.so` at runtime with no `LD_LIBRARY_PATH` and no `meson install`.
Keep the two exports set in any shell that runs `cargo build` / `test` /
the daemon; `FLUX_BUILD_DIR` is what selects the in-tree library. If Cargo
reports an undefined `flux_*` symbol, rebuild optics (`meson compile -C
../optics/build`) and re-run.

The contributor build uses Meson's `debugoptimized` profile. The Panel is a
CPU renderer, so an unoptimized (`buildtype=debug`) `libflux` can make normal
candidate navigation exceed a display-frame budget even when the Rust daemon
uses Cargo's release profile. To upgrade an existing build tree, run:

```bash
meson configure ../optics/build --buildtype=debugoptimized
meson compile -C ../optics/build
```

**Installed (optional).** If you prefer a system-wide flux, `meson install`
into a prefix on `PKG_CONFIG_PATH` and unset `FLUX_BUILD_DIR` (or set
`FLUX_USE_INSTALLED=1`) so pkg-config resolves the installed `flux.pc`
instead of the build tree.

## Build Typio

Build the debug daemon:

```bash
cargo build -p typio-host --bin typio
```

Build the release daemon:

```bash
meson setup ../optics/build-release ../optics -Dtext=true --buildtype=release
meson compile -C ../optics/build-release
export FLUX_BUILD_DIR="$PWD/../optics/build-release"
cargo build --release -p typio-host --bin typio
```

Build the CLI:

```bash
cargo build -p typioctl
```

Build and run the graphical settings application. Its Iris and Lens bindings
discover the same sibling optics build tree automatically; explicit variables
are useful when more than one optics checkout exists:

```bash
export LENS_BUILD_DIR="$PWD/../optics/build"
export LENS_SOURCE_DIR="$PWD/../optics"
export IRIS_BUILD_DIR="$PWD/../optics/build"
export IRIS_SOURCE_DIR="$PWD/../optics"
cargo run -p typio-settings
```

Build the framework and ABI tooling explicitly when touching engine contracts:

```bash
cargo check -p typio-abi
cargo check -p typio-core
cargo check -p typio-vet
```

Cargo features:

| Feature | Default | When to use it |
|---|---:|---|
| `wayland` | yes | Wayland input-method frontend, flux-backed Panel, and voice capture via PipeWire |
| `systray` | yes | StatusNotifierItem tray over D-Bus via zbus |

Disable default features only when isolating a non-Wayland Rust subsystem:

```bash
cargo test -p typio-host --no-default-features
```

## Run the Daemon

Run the debug binary:

```bash
./target/debug/typio --verbose
```

For engine work, point the daemon at one or more manifest directories.
`--engine-dir` is repeatable and takes highest precedence. Pass the
directory that contains `typio-engine-*.toml`; scanning is flat and does not
recurse. Manifest locations differ per engine — see the table below:

```bash
./target/debug/typio -v \
  --engine-dir ../typio-engines/typio-engine-compose \
  --engine-dir ../typio-engines/typio-engine-rime/build \
  --engine-dir ../typio-engines/typio-engine-mozc/build \
  --engine-dir ../typio-engines/typio-engine-sherpa/build
```

Equivalently, set the colon-separated `$TYPIO_ENGINE_PATH` once:

```bash
export TYPIO_ENGINE_PATH="$PWD/../typio-engines/typio-engine-compose:\
$PWD/../typio-engines/typio-engine-rime/build:\
$PWD/../typio-engines/typio-engine-mozc/build:\
$PWD/../typio-engines/typio-engine-sherpa/build"
./target/debug/typio -v
```

The daemon auto-loads only from the system engine directory. `--engine-dir`
and `$TYPIO_ENGINE_PATH` are explicit development/test opt-ins; no per-user
engine directory is scanned by default. See
[ADR-0025](../adr/0025-engine-discovery-search-path.md).

## Load Engines

`typio` starts without engines. Without a keyboard engine it has nothing to
convert keystrokes with; without a voice engine the voice push-to-talk path has
nothing to transcribe with. Build an engine and pass its manifest directory to
exercise input conversion or voice input.

The keyboard engines that ship as siblings under `typio-engines/`:

| Engine | Build system | Manifest path | Languages |
|---|---|---|---|
| `typio-engine-compose` | Cargo | `typio-engine-compose/typio-engine-compose.toml` | Latin with compose-key picker |
| `typio-engine-rime` | Meson (needs `librime`, `libcurl`) | `typio-engine-rime/build/typio-engine-rime.toml` | Chinese (zh) |
| `typio-engine-mozc` | Meson (needs Mozc depot) | `typio-engine-mozc/build/typio-engine-mozc.toml` | Japanese (ja) |

The voice engines that ship as siblings under `typio-engines/`:

| Engine | Build system | Manifest path | Languages |
|---|---|---|---|
| `typio-engine-sherpa` | Meson (needs `sherpa-onnx`, `libcurl`) | `typio-engine-sherpa/build/typio-engine-sherpa.toml` | Multilingual (mul) |
| `typio-engine-whisper` | Meson (needs whisper.cpp) | `typio-engine-whisper/build/typio-engine-whisper.toml` | Multilingual (mul) |

Build the Cargo engine with `cargo build --release` and the Meson engines
with `meson setup build && meson compile -C build` from their own roots,
e.g.:

```bash
cargo build --release --manifest-path ../typio-engines/typio-engine-compose/Cargo.toml
meson setup ../typio-engines/typio-engine-rime/build ../typio-engines/typio-engine-rime       # first time
meson compile -C ../typio-engines/typio-engine-rime/build
meson setup ../typio-engines/typio-engine-sherpa/build ../typio-engines/typio-engine-sherpa   # first time
meson compile -C ../typio-engines/typio-engine-sherpa/build
```

Then point the daemon at the directory that contains the manifest (note the
`/build` suffix for Meson engines):

```bash
./target/debug/typio -v --engine-dir ../typio-engines/typio-engine-compose
./target/debug/typio -v --engine-dir ../typio-engines/typio-engine-rime/build
./target/debug/typio -v --engine-dir ../typio-engines/typio-engine-sherpa/build
```

## Run Tests

Run the Cargo suite:

```bash
cargo test -p typio-host
cargo test -p typioctl
cargo test -p typio-client -p typio-settings
```

Run one test:

```bash
cargo test -p typio-host service::tests::hello_reports_protocol_and_capabilities
```

See [Testing](testing.md) for test ownership rules and common Cargo test
commands.

## Install

Install the already-built Cargo binary, systemd user service, icons, and
example configs:

```bash
cargo build --release -p typio-host --bin typio
cargo build --release -p typioctl
cargo build --release -p typio-settings
cargo xtask install --prefix /usr/local
```

Preview the install plan without writing files:

```bash
cargo xtask install --prefix /usr/local --dry-run
```

Remove installed files:

```bash
cargo xtask uninstall --prefix /usr/local
```

## Icons in Development

The system tray reports `IconName` and `IconThemePath` over D-Bus. During
development, `IconThemePath` points to `data/icons/hicolor/`, so most panels
find custom icons without installation.

If the panel ignores `IconThemePath`, install the icons into your user icon
theme:

```bash
mkdir -p ~/.local/share/icons/hicolor/scalable/apps
cp data/icons/hicolor/scalable/apps/*.svg ~/.local/share/icons/hicolor/scalable/apps/
gtk-update-icon-cache ~/.local/share/icons/hicolor 2>/dev/null || true
```

## See Also

- [Testing](testing.md)
- [Code Style](code-style.md)
- [Engine Discovery Reference](../reference/engine-discovery.md)
