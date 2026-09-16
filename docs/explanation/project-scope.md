# Project Scope: Daemon, Runtime, Engine Protocol, and Clients

The Typio workspace contains the Linux daemon, its Rust runtime, shared engine
contracts, a conformance tool, and TIP clients. Keeping them together allows a
protocol change and all of its consumers to land atomically without collapsing
their responsibility boundaries.

## Workspace Layers

| Layer | Role | Production boundary |
|---|---|---|
| `typio-daemon` | Wayland daemon, Panel, tray, audio capture, engine discovery, TIP server | Wayland, D-Bus, PipeWire, UDS |
| `typio-runtime` | Rust runtime: registry, config, input contexts, process supervision, voice state | Internal Rust API only |
| `typio-engine-manifest` | Owned manifest parsing and executable path resolution | `typio-engine-*.toml` |
| `typio-engine-protocol` | Typed engine frames and messages | Private inherited file descriptor |
| `typio-engine-check` | Black-box manifest, protocol, behavior, and resource checks | Starts the real worker process |
| `typio-control` / `typio-settings` | CLI and graphical settings control surfaces | TIP over a filesystem UDS |

The dependency direction is:

| From | To | Through |
|---|---|---|
| engine executable | `typio-runtime` | Typio Engine Protocol on the private inherited descriptor |
| `typio-runtime` | `typio-daemon` | in-workspace Rust API; the runtime is embedded as a library |
| `typio-daemon` | Wayland, Panel, audio | the platform surfaces it adapts |
| `typioctl` (`typio-control`), `typio-settings` | `typio-client` | in-workspace Rust API |
| `typio-client` | `typio-daemon` | TIP over a filesystem UDS |
| `typio-engine-check` | engine executable | manifest plus Typio Engine Protocol |

There is no engine plugin ABI, no shared engine library loaded into the daemon,
and no in-process engine adapter. A C or C++ engine may compile helper code into
its own executable, but compatibility is defined only by the manifest and the
wire protocol.

## The Two IPC Contracts

Typio intentionally has two IPC channels with different audiences:

| Contract | Peers | Discovery | Purpose |
|---|---|---|---|
| Typio Engine Protocol | daemon runtime ↔ one supervised engine | private inherited file descriptor | lifecycle, key/audio requests, composition, mode, availability, schema |
| TIP | daemon ↔ CLI/settings/third-party client | socket path under the user runtime directory | configuration, status, engine selection, commands, events |

An engine cannot connect to TIP as a replacement for its worker channel, and a
settings client never speaks Engine Protocol.

## Daemon Ownership

`typio-daemon` adapts operating-system capabilities to owned runtime values:

| Capability | Daemon responsibility |
|---|---|
| Keyboard input | Convert Wayland grab events into typed key events |
| Text output | Translate owned commit/composition events into text-input transactions |
| Popup UI | Position and render the CPU-canvas Panel over shared memory |
| Audio | Capture 16 kHz mono samples with PipeWire and feed a Rust voice session |
| Engine discovery | Scan manifests, negotiate capabilities, register worker argv |
| External control | Serve TIP and broadcast state changes |
| Desktop integration | SNI tray, systemd lifecycle, notifications |

The daemon owns one instance of the runtime state on its reactor thread.
Synchronous host subsystems share that instance on the same thread; worker
threads and session-bus callbacks never retain registry pointers, and instead
send typed events or hold owned process handles.

## Runtime Ownership

`typio-runtime` is platform-neutral and knows nothing about Wayland, D-Bus, panel
rendering, or microphone selection. It owns:

- engine registry state and language/engine switching;
- process launch, framed requests, deadlines, poisoning, and recovery;
- typed configuration plus static and engine-published schema;
- per-client input contexts and ordered output events;
- voice buffering, inference state, and completion signaling.

Its public Rust API is an internal workspace interface, not a separately
versioned SDK. The external engine compatibility surface is
`typio-engine-protocol`.

## The Conformance Tool

`typio-engine-check` is the black-box gate at the real process boundary. It
reads the same manifests as the daemon, starts the declared executable on its
own private protocol channel, and checks protocol conformance, basic engine
behavior, and resource placement. It never loads engine code itself, so it can
validate an engine in any language.

## Security Boundary

Engines are separate processes because they parse complex dictionaries and
models, may use large native dependencies, and can fail independently. The
private channel prevents accidental discovery by unrelated processes; frame
and payload limits bound hostile input. Process isolation is not a complete
sandbox, so package trust and the sandbox roadmap remain relevant.

See [ADR-0046](../adr/0046-engine-protocol-only-runtime.md), the
[Engine Discovery Reference](../reference/engine-discovery.md), and the
[Interface Stability Reference](../reference/stability.md).
