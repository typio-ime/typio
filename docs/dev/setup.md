# Developer Setup

This document is for contributors who modify Typio source code.

## Quick Start

All commands in this document run from the Typio repository root
unless a block says otherwise.

[README.md](../../README.md#building) holds the canonical build contract for
this repository: the native Optics libraries are built from the sibling Optics
checkout, and the Rust bindings locate them through `OPTICS_BUILD_DIR` and
`OPTICS_SOURCE_DIR`. A development loop differs from that release recipe in two
ways only: the Optics tree is built `debugoptimized` into `../optics/build`, and
the binaries under test are the debug ones.

```bash
# one-time per optics checkout:
meson setup ../optics/build ../optics -Dtext=true --buildtype=debugoptimized
meson compile -C ../optics/build

export OPTICS_BUILD_DIR="$PWD/../optics/build"
export OPTICS_SOURCE_DIR="$PWD/../optics"
export OPTICS_STATIC=1

cargo build -p typio-daemon --bin typio
cargo test -p typio-daemon
./target/debug/typio --verbose
```

The Optics export variables must be present in every shell that runs `cargo
build`, `test`, or `run`; the repository ships no committed copy of them.
`OPTICS_BUILD_DIR` and `OPTICS_SOURCE_DIR` cover every Optics layer (`gfx`,
`text`, `ui`, `window`, and `app`), including `typio-settings`; per-library
`OPTICS_GFX_*` / `OPTICS_TEXT_*` / `OPTICS_UI_*` / `OPTICS_WINDOW_*` /
`OPTICS_APP_*` variables remain supported as fine-grained overrides.

Typio's `optics` dependency enables Optics's `static` feature, so when the
sibling build tree ships static archives (`lib*.a`) the Rust build scripts link
them into the Typio binaries (only OS/system libraries remain dynamic, with no
build-tree rpath). `OPTICS_STATIC=1` in the environment is an equivalent
override; when linking against shared Optics libraries (`OPTICS_DYNAMIC=1` or a
shared-only build tree), the Typio build scripts bake `-Wl,-rpath` entries to
the build tree. `typio-runtime`,
`typio-engine-protocol`, `typio-engine-manifest`, `typio-engine-check`, `typio-client`,
`typio-control`, and `typio-settings` are Cargo workspace members in this
repository, so runtime, engine-contract, client, GUI, CLI, and daemon changes
build together.

The shipping daemon is the Rust `typio` binary from `crates/typio-daemon`.

## Prerequisites

Install these from your system package manager:

- Rust 1.85+ and Cargo
- Meson 1.0+, Ninja 1.10+, and `glslangValidator` to build the Optics C libraries from source
- C23 compiler and `pkg-config`
- Wayland client libraries and `xkbcommon`
- Vulkan headers (Optics `gfx` is Vulkan-rooted; the host uses only its CPU canvas, but
  the headers are required to build liboptics_gfx)
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
    ├── libs/gfx/, libs/text/     # C canvas and text shaping libraries; build in-tree and point OPTICS_BUILD_DIR at it
    ├── libs/window/, libs/ui/    # settings window and widget toolkit
    ├── libs/app/                 # application shell binding UI to a window
    └── crates/                   # unified Cargo workspace: the `optics` facade plus `optics-*` layer crates
```

Typio resolves its runtime, engine-protocol, manifest, vet, command-line, and
settings crates from local workspace paths. Native optics libraries are
sibling dependencies found through pkg-config by the `-sys` crates underneath
the single `optics` facade dependency.

## optics-gfx (C library)

`optics-gfx-sys` does not vendor the C source; its build script locates `liboptics_gfx`
through pkg-config. The contributor workflow builds Optics in its own tree and
points the build script at it, so no system install is needed.

**In-tree build (default).** Build Optics once, then point `optics-gfx-sys` at the
build tree. `meson setup` is one-time per checkout; `meson compile` rebuilds
on demand:

```bash
meson setup ../optics/build ../optics -Dtext=true --buildtype=debugoptimized
meson compile -C ../optics/build

export OPTICS_BUILD_DIR="$PWD/../optics/build"
export OPTICS_SOURCE_DIR="$PWD/../optics"   # optional: bindgen from this checkout
```

The Optics `-sys` crates prepend the build tree's `meson-uninstalled/` to
`PKG_CONFIG_PATH`. Static archives (`lib*.a`) are preferred automatically while
system libraries stay dynamic; set `OPTICS_STATIC=1` to require static archives
or `OPTICS_DYNAMIC=1` to force shared-library linking (for which Typio's build
scripts bake `-Wl,-rpath` entries so binaries run without `LD_LIBRARY_PATH` or
`meson install`). Keep the Optics export variables set in any shell that runs
`cargo build`, `cargo test`, or the daemon; `OPTICS_BUILD_DIR` (or
`OPTICS_GFX_BUILD_DIR`) is what selects the in-tree library. If Cargo reports an
undefined `optics_gfx_*` symbol, rebuild optics (`meson compile -C ../optics/build`)
and re-run.

The contributor build uses Meson's `debugoptimized` profile. The Panel is a
CPU renderer, so an unoptimized (`buildtype=debug`) `liboptics_gfx` can make normal
candidate navigation exceed a display-frame budget even when the Rust daemon
uses Cargo's release profile. To upgrade an existing build tree, run:

```bash
meson configure ../optics/build --buildtype=debugoptimized
meson compile -C ../optics/build
```

**Installed (optional).** If you prefer a system-wide Optics install, `meson install`
into a prefix on `PKG_CONFIG_PATH` and unset `OPTICS_BUILD_DIR` (or set
`OPTICS_GFX_USE_INSTALLED=1`) so pkg-config resolves the installed `optics-gfx.pc`
instead of the build tree.

## Build Typio

Build the debug daemon:

```bash
cargo build -p typio-daemon --bin typio
```

Build the release daemon:

```bash
meson setup ../optics/build-release ../optics -Dtext=true --buildtype=release
meson compile -C ../optics/build-release
export OPTICS_BUILD_DIR="$PWD/../optics/build-release"
export OPTICS_SOURCE_DIR="$PWD/../optics"
export OPTICS_STATIC=1
cargo build --release -p typio-daemon --bin typio
```

Build the CLI:

```bash
cargo build -p typio-control --bin typioctl
```

Run the CLI straight from the source tree when iterating on it (`typioctl`
answers only while a daemon is running):

```bash
cargo run -p typio-control --bin typioctl -- daemon status
cargo run -p typio-control --bin typioctl -- engine list
```

Build and run the graphical settings application. With `OPTICS_BUILD_DIR` and
`OPTICS_SOURCE_DIR` exported, every layer of the `app`, `window`, `ui`, `text`,
and `gfx` stack resolves from the sibling optics build tree:

```bash
cargo run -p typio-settings
```

Build the runtime and engine-contract tooling explicitly when touching engine
contracts:

```bash
cargo check -p typio-runtime
cargo check -p typio-engine-protocol
cargo check -p typio-engine-manifest
cargo check -p typio-engine-check
```

Cargo features:

| Feature | Default | When to use it |
|---|---:|---|
| `wayland` | yes | Wayland input-method frontend, `optics::gfx`-backed Panel, and voice capture via PipeWire |
| `systray` | yes | StatusNotifierItem tray over D-Bus via zbus |

Disable default features only when isolating a non-Wayland Rust subsystem:

```bash
cargo test -p typio-daemon --no-default-features
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
  --engine-dir ../typio-engines/typio-engine-compose
```

Equivalently, set the colon-separated `$TYPIO_ENGINE_PATH` once:

```bash
export TYPIO_ENGINE_PATH="$PWD/../typio-engines/typio-engine-compose"
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

The engine repositories live under the sibling `typio-engines/` umbrella.
Protocol compatibility is per engine revision, not implied by directory
presence:

| Engine | Modality | Current compatibility |
|---|---|---|
| `typio-engine-compose` | Keyboard | Protocol-native; supported |
| `typio-engine-rime` | Keyboard | ABI-based revision requires migration |
| `typio-engine-mozc` | Keyboard | ABI-based revision requires migration |
| `typio-engine-sherpa` | Voice | ABI-based revision requires migration |
| `typio-engine-whisper` | Voice | ABI-based revision requires migration |

Build the protocol-native Cargo engine from the Typio checkout:

```bash
cargo build --release --manifest-path ../typio-engines/typio-engine-compose/Cargo.toml
```

Engine executables speak Typio Engine Protocol over their inherited fd 3.
They do not link `typio-runtime` or a Typio shared library. Rust engines should
depend on `typio-engine-protocol`; non-Rust engines implement the documented
framing and messages directly or use an engine-local source adapter.

The current C/C++ sibling revisions still link the retired ABI and therefore
cannot be loaded by this runtime. Their migration belongs in their respective
repositories: remove the shared-library dependency, compile a direct worker
executable, and validate its manifest with `typio-engine-check`. Typio deliberately
does not ship an ABI compatibility layer.

Then point the daemon at the directory that contains the manifest:

```bash
./target/debug/typio -v --engine-dir ../typio-engines/typio-engine-compose
```

## Run Tests

Run the Cargo suite:

```bash
cargo test -p typio-daemon
cargo test -p typio-control
cargo test -p typio-runtime -p typio-engine-protocol -p typio-engine-manifest
cargo test -p typio-engine-check -p typio-client -p typio-settings
```

Run one test:

```bash
cargo test -p typio-daemon service::tests::hello_reports_protocol_and_capabilities
```

See [Testing](testing.md) for test ownership rules and common Cargo test
commands.

## Install

Install the already-built Cargo binary, systemd user service, icons, and
example configs:

```bash
cargo build --release -p typio-daemon --bin typio
cargo build --release -p typio-control --bin typioctl
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
