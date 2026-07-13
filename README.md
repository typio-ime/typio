# typio

**Typio for Linux** — a Wayland-native input method host for the
[Typio](https://github.com/) input method framework. Installs the `typio`
daemon, the `typioctl` command-line client, and the `typio-settings` graphical
settings application.

> Currently Wayland-only (`text-input-v2` / `input-method-v2`). X11 is not
> supported and not planned — this host targets the modern Wayland desktop.

It embeds the workspace `typio-core` crate and provides the platform adapter layer:
the Wayland text-input/input-method v2 client, virtual-keyboard bridge,
the candidate Panel (CPU-rendered over `wl_shm`; see `docs/adr/0040-cpu-canvas-render-shm-buffers.md`), the UDS control socket,
the StatusNotifierItem tray, and PipeWire voice capture. It translates
Wayland events into typio-core abstractions and drives typio-core's callbacks
back onto the compositor. (The old D-Bus status interface was removed in
ADR-0008; the tray speaks SNI over D-Bus via zbus when the `systray`
Cargo feature is enabled.)

Engine discovery is host-owned: at startup `typio` scans
`<datadir>/typio/engines` for `typio-engine-*.toml` manifests and registers
direct worker processes with typio-core. Core itself contains no engine search
paths.

## Building

Requires Wayland, xkbcommon, fontconfig/harfbuzz/freetype, PipeWire for voice
capture, and the optics graphics stack. `typio-core`, `typio-abi`,
`typio-vet`, `typioctl`, and `typio-settings` are workspace crates in this
repository.

The host build is Cargo. `flux` is still a native C library, so build the
sibling flux checkout first until flux has its own Cargo-native library
build.

```bash
# Build the native dependencies first (from the Typio repo root).
meson setup ../optics/build-release ../optics \
  -Dtext=true --buildtype=release    # one-time per optics checkout
meson compile -C ../optics/build-release

export FLUX_BUILD_DIR="$PWD/../optics/build-release"
export FLUX_SOURCE_DIR="$PWD/../optics/libs/flux"
export LENS_BUILD_DIR="$PWD/../optics/build-release"
export LENS_SOURCE_DIR="$PWD/../optics"
export IRIS_BUILD_DIR="$PWD/../optics/build-release"
export IRIS_SOURCE_DIR="$PWD/../optics"
cargo build --release -p typio-host
cargo build --release -p typioctl
cargo build --release -p typio-settings
cargo test -p typio-host
```

The binaries are produced at `target/release/typio`,
`target/release/typioctl`, and `target/release/typio-settings`. Install them
along with the systemd service, desktop metadata, icons, and example configs
with `cargo xtask install`.

See [`docs/dev/setup.md`](docs/dev/setup.md) for the full setup steps and
additional options.

Cargo features: `--features systray` enables the StatusNotifierItem tray
(via zbus). Voice capture ships with the default `wayland` feature and runs
PipeWire's `pw-record` as a subprocess; voice engines run as worker processes
at runtime.

## Running

```bash
typio --verbose                # run the daemon with debug logging
```

`typio` is the daemon. Inspecting and controlling a running instance (engines,
config, status) is the job of the workspace `typioctl` client, which talks to
the daemon over its UDS socket.

Launch the graphical settings application after starting the daemon:

```bash
typio-settings
```

Installed packages start the daemon through the systemd user service:

```bash
systemctl --user enable --now typio.service
journalctl --user -u typio -f
```

Engines are discovered from the system engine directory
`<prefix>/<datadir>/typio/engines`. Build a sibling engine such as
[compose](../typio-engines/typio-engine-compose) (`cargo build --release`) or
[rime](../typio-engines/typio-engine-rime) (`meson setup build && meson compile -C build`)
and install its `typio-engine-*.toml` into that directory. For development
and testing, pass `--engine-dir DIR` or set `TYPIO_ENGINE_PATH`.

Control it from a separate terminal with the [typioctl](crates/typioctl) client.
