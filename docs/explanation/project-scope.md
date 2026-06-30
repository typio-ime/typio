# Project Scope: Host, Framework, ABI, Vet, and CLI

This document clarifies which layer owns which responsibility inside the
Typio workspace. The repository contains the Linux host daemon, the
platform-neutral framework library, the shared engine ABI crate, and the engine
vetting tool, plus the command-line client. Keeping them in one workspace makes
cross-layer changes atomic, but it does not erase the design boundary between
the layers.

## The Workspace Layers

| | `typio-host` | `typio-core` | `typio-abi` | `typio-vet` | `typioctl` |
|---|---|---|---|---|---|
| **Role** | Linux/Wayland host daemon | Platform-neutral framework | Shared Rust ABI types | Engine contract checker | Command-line TIP client |
| **Output** | `typio` binary | `libtypio.so` and `rlib` | `rlib` | `typio-vet` binary and test harness | `typioctl` binary |
| **Knows about** | Wayland, Vulkan/Flux, D-Bus, PipeWire, Linux filesystem | engines, input contexts, config schema, engine registry | `#[repr(C)]` engine structs and enums | engine lifecycle, ABI invariants, resource packaging | daemon TIP methods and CLI presentation |
| **Does not know about** | how an engine processes a key; what Rime schema is active | compositor behavior, GPU resources, audio hardware | runtime state or host behavior | compositor, panel rendering, daemon event loop | Wayland, rendering, engine internals |
| **Contains engines** | No; discovers manifests and registers engine argv | No; owns engine routing, contracts, and worker transport | No | No; loads engines only for tests | No |

The dependency direction is intentionally narrow:

```text
typio-host ──▶ typio-core ──▶ typio-abi
typio-vet  ───────────────▶ typio-abi
typioctl   ───────────────▶ TIP/UDS daemon protocol
engines    ───────────────▶ typio-abi
```

`typio-core` never reaches back into the host except through callbacks the host
registers. Engines remain separate packages and communicate with the daemon as
out-of-process workers.

## The Boundary

The contract between host and framework is defined by three surfaces in
typio-core's public headers:

### Surface 1: Instance lifecycle (`typio/runtime/instance.h`)

The host creates a `TypioInstance`, provides engine directories and an engine
discovery callback, then drives init and shutdown. typio-core never scans engine
paths; it calls back into the host's `TypioPluginLoaderFunc` once per engine
directory, and the host does manifest parsing, capability negotiation, and IPC
registration.

### Surface 2: Input context (`typio/abi/input_context.h`)

The host creates contexts, feeds `TypioKeyEvent` structs in, and registers
callbacks for text output. When an engine produces text, it calls
`typio_input_context_commit` or `typio_input_context_set_composition`; typio-core
fires the host's callbacks synchronously on the same call stack; the host
translates those into Wayland protocol calls (`commit_string`,
`set_preedit_string`, etc.).

This is the bidirectional boundary: keys flow **in** from the compositor through
typio-core to the engine; text and compositions flow **out** from the engine
through typio-core back to the compositor.

### Surface 3: Observer callbacks (`typio/runtime/instance.h`)

The host registers callbacks for engine activation, mode changes, and status
icon updates. typio-core fires these when its internal state changes; the host
updates the tray, panel, and IPC event subscribers.

```
┌──────────────────────────────────────────────────────────┐
│  Engine Process                                           │
│  Implements Typio Engine Protocol                          │
│  Emits COMMIT / COMPOSITION response lines                 │
└──────────────┬──────────────────────┬────────────────────┘
	               │ engine request       │ engine responses
               ▼                      │
┌──────────────────────────────────────┴────────────────────┐
│  typio-core + typio-abi                                      │
│  Instance, Registry, InputContext, ABI types                │
│  Engine lifecycle, worker transport, key routing, config   │
│  No display server, no GPU, no audio                       │
└──────────────┬─────────────────────────────────────────────┘
               │ host API + observer callbacks
               ▼
┌────────────────────────────────────────────────────────────┐
│  typio-host                                                │
│  Wayland input-method, Vulkan panel, event loop            │
│  Manifest discovery, tray, IPC, voice capture, config watch│
└────────────────────────────────────────────────────────────┘
```

## What typio-host Owns

### OS capability abstraction

`typio-host` adapts the Linux/Wayland desktop to the portable interfaces
typio-core defines:

| OS capability | Host module | Abstracted as |
|---|---|---|
| Keyboard input | Wayland keyboard grab → `TypioKeyEvent` | Input context key feed |
| Text output | Engine callbacks → `zwp_input_method_v2` commit/preedit | Compositor protocol |
| Popup positioning | `zwp_input_popup_surface_v2` | Panel surface |
| GPU rendering | Vulkan via flux canvas library | Candidate panel |
| Audio capture | PipeWire → `TypioVoiceSession.feed_audio` | Voice input |
| System tray | D-Bus StatusNotifierItem | Engine status indicator |
| Desktop notifications | D-Bus `org.freedesktop.Notifications` | Health alerts |
| External control | UDS + JSON-RPC 2.0 (TIP v1) | IPC bus |
| Engine discovery | Manifest parsing + capability negotiation | Registry registration |
| Config persistence | File watch + debounced reload | Runtime config |
| Per-app identity | Store/restore engine and mode by `app_id` | Instance state |

### UX consistency

Because `typio-host` sits between the operating system and the user, it bears
a responsibility that neither layer above (compositor) nor below (typio-core)
can fulfill: **ensuring a consistent, responsive user experience across all
the surfaces the user actually sees and touches.**

This is not cosmetic polish tacked on at the end. The UX responsibility is
structural:

- **Input responsiveness.** The event loop is single-threaded. Every GPU
  frame, D-Bus dispatch, config reload, and voice audio buffer competes with
  key processing for the same loop tick. The host must bound every non-input
  operation so a keypress never waits behind a swapchain rebuild, a glyph
  upload, or a compositor stall. See [Event Loop Scheduling](event-loop-scheduling.md)
  and [Vulkan and Flux Rendering](vulkan-flux-rendering.md).

- **Visual consistency.** The candidate panel, preedit decoration, tray icon,
  and mode indicator must all reflect the same state at the same time. The
  `TypioStateController` observer pattern ensures every surface reads from one
  source of truth. See [Control Surfaces](control-surfaces.md).

- **Crash recovery.** A compositor lock, DPMS-off, or system suspend must not
  corrupt committed text or freeze the input path. The focus controller derives
  state from facts every step, so recovery is the same code path as normal
  operation.
  See [Input-Method Session](input-method-session.md).

- **Cross-engine smoothness.** Switching engines must clear stale preedit and
  panel state before the new engine activates, so no ghost text survives an
  engine boundary. Shortcut bypass, modifier buffering, and key epoch fencing
  prevent keys from leaking between engines or between the IME and the
  application.

These are not features of typio-core; they are properties of how the host bridges
OS capabilities to the framework. A different host (macOS, Windows, Android)
would face different OS surfaces but the same UX invariants.

## What typio-core Owns

typio-core is the platform-neutral core. It provides:

- `TypioInstance` — lifecycle, config, per-app identity
- `TypioRegistry` — engine listing, activation, switching
- `TypioInputContext` — key dispatch to engines, composition aggregation
- Engine protocol and host-facing C ABI glue
- Config schema — shared key vocabulary for hosts, engines, and control panels

typio-core does not contain engines, does not open shared libraries, does not
talk to any display server, and does not render pixels. Any code that would
only make sense on one operating system belongs in the host.

## What typio-abi Owns

`typio-abi` contains the Rust representation of engine-facing ABI types:

- `#[repr(C)]` structs and enums shared by Rust engines and vet tests
- type definitions that must stay bit-compatible with `include/typio/abi/`
- no registry, no engine policy, no host callbacks, and no IO

Rust engines depend on `typio-abi` rather than linking the full framework.
This keeps engine builds small and prevents engine code from reaching into
host or framework internals.

## What typio-vet Owns

`typio-vet` is a consumer of the engine contract, not part of the runtime
daemon. It owns:

- ABI and lifecycle conformance checks for engine executables
- a mock host harness for Rust engine tests
- resource checks such as icon naming and placement

It may encode expectations about engine behavior, but it must not become a
second implementation of host policy. If a check requires Wayland state,
panel state, IPC state, or daemon scheduling behavior, it belongs in host
tests instead.

## What typioctl Owns

`typioctl` is the command-line client for the daemon's TIP/UDS control surface.
It owns:

- command-line parsing and output formatting
- user-facing resource/verb command names
- client-side request sequencing over the daemon socket
- mapping CLI commands to daemon RPC methods

It must not own daemon policy. If a command needs a new source of truth, schema
entry, engine switch rule, or event payload, that contract is added to the host
protocol first and the CLI follows it.

## What Belongs Where

When deciding where a change belongs, apply these tests:

| Question | If yes, it belongs in `typio-host` |
|---|---|
| Does it require a Wayland protocol object? | Yes |
| Does it require GPU rendering or a GPU resource? | Yes |
| Does it require D-Bus, PipeWire, inotify, or epoll? | Yes |
| Does it affect the panel layout, theme, or visual appearance? | Yes |
| Does it affect how fast a keypress becomes a visible result? | Yes |
| Does it affect how the tray icon or IPC surface reports state? | Yes |
| Does it discover an engine manifest or register an engine executable? | Yes |

| Question | If yes, it belongs in `typio-core` |
|---|---|
| Does it affect how keys are routed to engines? | Yes |
| Does it affect the composition data model? | Yes |
| Does it affect engine lifecycle (init, destroy, focus, reset)? | Yes |
| Does it define a config key that any host or engine must understand? | Yes |
| Does it add a new engine capability or vtable method? | Yes |

| Question | If yes, it belongs in `typio-abi` |
|---|---|
| Is it a Rust representation of an engine-facing C ABI type? | Yes |
| Must Rust engines compile against it without linking typio-core? | Yes |
| Can it be represented without runtime behavior or IO? | Yes |

| Question | If yes, it belongs in `typio-vet` |
|---|---|
| Does it validate an engine executable or package? | Yes |
| Does it provide a mock host for engine unit tests? | Yes |
| Does it report ABI, behavior, or resource conformance? | Yes |

| Question | If yes, it belongs in `typioctl` |
|---|---|
| Does it change command-line syntax or output rendering? | Yes |
| Does it map an existing daemon RPC to a user command? | Yes |
| Does it improve client-side socket error reporting? | Yes |

| Question | If yes, it belongs in an engine plugin |
|---|---|
| Does it implement a specific input method (Rime, Pinyin, voice)? | Yes |
| Does it process key events and produce text? | Yes |
| Does it maintain its own dictionary, model, or schema? | Yes |

## Common Confusion Points

**"I want to add a new input method."** Write an engine plugin against
`typio/abi/abi.h`. Neither `typio-host` nor typio-core needs to change.

**"I want the panel to show a new kind of content."** This is `typio-host`.
The panel content model (`TypioPanelContent`) is GPU-free and testable, but
the surface, rendering, and positioning are Wayland-specific.

**"I want to change how engines are switched (Ctrl+Shift, next/prev)."** The
trigger mechanism (keyboard shortcut detection, modifier buffering) is in the
host. The registry operation (`next_keyboard`) is in typio-core.

**"I want to add a new config key."** If the key is consumed by engines, define
it in typio-core's config schema. If the key controls a host behavior (panel
font, tray visibility, GPU options), it belongs in `typio-host` runtime
config.

**"I want to port Typio to macOS."** Write a new host that links typio-core,
feeds keys, handles text output, and renders a native panel. typio-core,
typio-abi, typio-vet, and engine plugins stay the same unless the shared
contract itself needs to change.

## See also

- [Panel Architecture](panel-architecture.md) — the UI surface typio renders.
- [Control Surfaces](control-surfaces.md) — tray, IPC bus, state controller.
- [Event Loop Scheduling](event-loop-scheduling.md) — how the event loop preserves responsiveness.
- [Input-Method Session](input-method-session.md) — session lifecycle and recovery paths.
- [Vulkan and Flux Rendering](vulkan-flux-rendering.md) — GPU rendering and performance.
- [Wayland Input Method Protocol](wayland-input-method.md) — the protocol layer.
