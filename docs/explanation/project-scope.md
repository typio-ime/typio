# Project Scope: Host, Runtime, Protocol, Vet, and Clients

The Typio workspace contains the Linux host, its Rust runtime, shared engine
contracts, a conformance tool, and TIP clients. Keeping them together allows a
protocol change and all of its consumers to land atomically without collapsing
their responsibility boundaries.

## Workspace Layers

| Layer | Role | Production boundary |
|---|---|---|
| `typio-host` | Wayland daemon, Panel, tray, audio capture, engine discovery, TIP server | Wayland, D-Bus, PipeWire, UDS |
| `typio-core` | Rust runtime: registry, config, input contexts, process supervision, voice state | Internal Rust API only |
| `typio-engine-manifest` | Owned manifest parsing and executable path resolution | `typio-engine-*.toml` |
| `typio-engine-protocol` | Typed engine frames and messages | Private inherited fd 3 |
| `typio-vet` | Black-box manifest, protocol, behavior, and resource checks | Starts the real worker process |
| TIP clients | Shared client, CLI, and graphical settings | TIP over a filesystem UDS |

The dependency direction is:

```text
engine executable ── Engine Protocol/fd 3 ──▶ typio-core ──▶ typio-host
                                                    │
typioctl ───────┐                                   └── Wayland / Panel / audio
typio-settings ─┴──▶ typio-client ── TIP/UDS ──▶ typio-host

typio-vet ── manifest + Engine Protocol ──▶ engine executable
```

There is no engine plugin ABI, `libtypio.so`, or daemon-side `dlopen`. A C or
C++ engine may compile helper code into its executable, but compatibility is
defined only by the manifest and wire protocol.

## The Two IPC Contracts

Typio intentionally has two IPC channels with different audiences:

| Contract | Peers | Discovery | Purpose |
|---|---|---|---|
| Typio Engine Protocol | daemon runtime ↔ one supervised engine | private inherited fd 3 | lifecycle, key/audio requests, composition, mode, availability, schema |
| TIP | daemon ↔ CLI/settings/third-party client | socket path under the user runtime directory | configuration, status, engine selection, commands, events |

An engine cannot connect to TIP as a replacement for its worker channel, and a
settings client never speaks Engine Protocol.

## Host Ownership

`typio-host` adapts operating-system capabilities to owned runtime values:

| Capability | Host responsibility |
|---|---|
| Keyboard input | Convert Wayland grab events into typed key events |
| Text output | Translate owned commit/composition events into text-input transactions |
| Popup UI | Position and render the CPU-canvas Panel over `wl_shm` |
| Audio | Capture 16 kHz mono samples with PipeWire and feed a Rust voice session |
| Engine discovery | Scan manifests, negotiate capabilities, register worker argv |
| External control | Serve TIP and broadcast state changes |
| Desktop integration | SNI tray, systemd lifecycle, notifications |

The host owns one `TypioInstance` on its reactor thread. Synchronous host
subsystems share it through `Rc<RefCell<_>>`. Worker threads and zbus callbacks
never retain registry pointers; they send typed events or use owned process
handles.

## Runtime Ownership

`typio-core` is platform-neutral and knows nothing about Wayland, D-Bus, panel
rendering, or microphone selection. It owns:

- engine registry state and language/engine switching;
- process launch, framed requests, deadlines, poisoning, and recovery;
- typed configuration plus static and EngineHello-published schema;
- per-client input contexts and ordered output events;
- voice buffering, inference state, and completion signaling.

Its public Rust API is an internal workspace interface, not a separately
versioned SDK. The external engine compatibility surface is
`typio-engine-protocol`.

## Security Boundary

Engines are separate processes because they parse complex dictionaries and
models, may use large native dependencies, and can fail independently. The
private channel prevents accidental discovery by unrelated processes; frame
and payload limits bound hostile input. Process isolation is not a complete
sandbox, so package trust and the sandbox roadmap remain relevant.

See [ADR-0046](../adr/0046-engine-protocol-only-runtime.md), the
[Engine Discovery Reference](../reference/engine-discovery.md), and the
[Interface Stability Reference](../reference/stability.md).
