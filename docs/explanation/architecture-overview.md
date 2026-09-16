# Runtime architecture

Typio has three deliberately different boundaries:

| Boundary | Transport | What crosses it |
|---|---|---|
| Compositor and the host daemon | Wayland | Input-method activation, text state, and keys; surfaces, popups, and commits |
| Host daemon and the runtime | owned Rust values inside one process | Focus, configuration, registry, and input-context operations |
| Runtime and an engine process | typed frames on a private inherited file descriptor | Lifecycle, key, audio, mode, and command transactions |
| `typioctl`, `typio-settings`, and third-party clients and the host daemon | TIP JSON-RPC on a Unix socket | Configuration, status, engine selection, commands, and events |

`typio-daemon` owns Wayland, the event loop, panel rendering, PipeWire capture,
system integration, engine discovery, and TIP. `typio-runtime` owns portable
runtime policy and state. Engine executables own language or speech logic and
cannot share pointers or crash the daemon address space.

The host/runtime edge is not a distribution interface. Both live in one
workspace and pass ordinary owned values — strings, vectors, boxes, and shared
handles — chosen by each side's thread model. No raw pointer, user-data
callback, or manually synchronized layout is required, and neither side has to
treat the other as an ABI.

The engine edge is the versioned Typio Engine Protocol. The host spawns a
manifest-declared executable, maps a private socket to the protocol's inherited
file descriptor, validates the worker's opening handshake, and exchanges
bounded typed requests and replies. The standard streams remain available for
logs. Discovery, activation, poisoning, respawning, and shutdown are all owned
by the engine backend, so no engine code ever runs on the daemon's thread.

TIP is unrelated to the engine protocol. It is an external control API for
clients such as `typioctl` and `typio-settings`; those clients never receive an
engine file descriptor or call runtime objects directly.

This split gives each compatibility promise one owner:

| Concern | Owner |
|---|---|
| Wayland object and serial lifetime | host |
| Config, language, registry, and context policy | runtime |
| Engine framing and payload encoding | `typio-engine-protocol` |
| Manifest parsing and path resolution | `typio-engine-manifest` |
| External control methods | TIP service |
| Engine conformance | `typio-engine-check` at the process boundary |

See [ADR-0046](../adr/0046-engine-protocol-only-runtime.md)
for why the former ABI was removed.
