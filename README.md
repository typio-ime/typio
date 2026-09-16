# typio

**Typio for Linux** — a Wayland-native input method host for the Typio input
method framework. Installs the `typio` daemon, the `typioctl` command-line
client, and the `typio-settings` graphical settings application.

> Currently Wayland-only (`text-input-v2` / `input-method-v2`). X11 is not
> supported and not planned — this host targets the modern Wayland desktop.

## Key capabilities

- **Wayland-native input method.** Speaks `zwp_input_method_v2` directly and
  drives a virtual keyboard, so it works on a stock Wayland desktop without a
  portal or an IBus compatibility layer.
- **Isolated engine processes.** Every engine is a worker process behind a typed
  protocol on a private descriptor: no `dlopen`, no shared library, no C ABI,
  and an engine crash cannot take the daemon with it (ADR-0046).
- **A CPU-rendered candidate Panel.** Candidates, mode indicators, and voice
  status share one input-popup surface rendered over shared memory, with no GPU
  device or per-frame readback in the Panel path (ADR-0040).
- **One control surface.** The `typioctl` client, the settings application, and
  any third-party integration all drive the daemon through the same
  length-prefixed JSON-RPC socket (TIP v3), so the CLI and the GUI cannot
  disagree about state (ADR-0008, ADR-0045).
- **An idle daemon that stays idle.** The event loop blocks on `poll(2)` with no
  periodic tick, so an unused input method costs no wakeups (ADR-0024).

## Building

Requires Wayland, xkbcommon, fontconfig/harfbuzz/freetype, PipeWire for voice
capture, and the optics graphics stack. `typio-runtime`,
`typio-engine-protocol`, `typio-engine-manifest`, `typio-engine-check`,
`typio-control`, `typio-client`, and `typio-settings` are workspace crates in
this repository.

The daemon build is Cargo. `flux` is still a native C library, so build the
sibling flux checkout first until flux has its own Cargo-native library build.

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
cargo build --release -p typio-daemon --bin typio
cargo build --release -p typio-control --bin typioctl
cargo build --release -p typio-settings
cargo test -p typio-daemon
```

The binaries are produced at `target/release/typio`,
`target/release/typioctl`, and `target/release/typio-settings`. Install them
along with the systemd service, desktop metadata, icons, and example configs
with `cargo xtask install`.

See [How to Package for Distribution](docs/how-to/package-for-distribution.md)
for installable layouts and a packaging checklist, and
[Developer Setup](docs/dev/setup.md) for the contributor loop.

Cargo features: `--features systray` enables the StatusNotifierItem tray (via
zbus). Voice capture ships with the default `wayland` feature and runs
PipeWire's `pw-record` as a subprocess; voice engines run as worker processes at
runtime.

## Running

```bash
typio --verbose                # run the daemon with debug logging
```

`typio` is the daemon. Inspecting and controlling a running instance (engines,
config, status) is the job of the workspace `typioctl` client, which talks to
the daemon over its UDS socket:

```bash
typioctl daemon status
typioctl engine list
```

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
`<prefix>/<datadir>/typio/engines`. Typio ships no engine: install an engine
package, or build one following
[How to Write an Engine](docs/how-to/write-an-engine.md) and install its
manifest into that directory. For development and testing, pass
`--engine-dir DIR` or set `TYPIO_ENGINE_PATH`.

Every engine is an isolated worker process. The daemon gives it a private fd 3
channel and speaks the typed Typio Engine Protocol; no engine is loaded as a
shared library and no host or engine C ABI is part of the architecture. TIP,
the UDS JSON-RPC protocol used by `typioctl` and `typio-settings`, is a separate
external client interface. See [ADR-0046](docs/adr/0046-engine-protocol-only-runtime.md).

Older C/C++ sibling-engine revisions that link the retired Typio ABI are not
compatible with this runtime. They must be migrated to self-contained protocol
workers; the daemon intentionally provides no compatibility shim.

## Documentation

- [Your First Typio Session](docs/tutorials/getting-started.md) — install,
  start, and type with an engine.
- [How-to Guides](docs/how-to/index.md) — packaging, configuration,
  troubleshooting, engine authoring.
- [Reference](docs/reference/index.md) — commands, configuration keys, the TIP
  protocol, the engine protocol.
- [Explanation](docs/explanation/index.md) — why the host is shaped this way.
- [Architecture Blueprints](docs/architecture/index.md) and the
  [ADR index](docs/adr/index.md) — the current design and the decisions behind it.
- [Contributing](CONTRIBUTING.md) and the
  [developer documentation](docs/dev/index.md) — building, testing, and
  releasing Typio.
