# Module Map

Contributor-facing index of Typio implementation coordinates: the Cargo
workspace crates, the responsibility of each, and the modules that own each
behaviour. Conceptual documents stay conceptual — see
[Where implementation coordinates live](#where-implementation-coordinates-live).

Each crate that needs coordinates gets its own `## Crate: <name>` section.

## Workspace Crates

The workspace members are declared in the root `Cargo.toml`.

| Crate | Binary | Responsibility |
|---|---|---|
| `crates/typio-daemon` | `typio` | Wayland input-method daemon: session and focus handling, keyboard policy, candidate Panel driving, tray surface, voice plumbing, and the TIP UDS server. |
| `crates/typio-host-platform` | — | Wayland and Flux platform integration: surfaces, input, SHM presentation, and text rasterisation. |
| `crates/typio-host-types` | — | Platform-neutral host state and policy types shared by the daemon and the platform layer. |
| `crates/typio-runtime` | — | Headless input-method state machine and engine-scheduling runtime; built as an `rlib` and embedded by the daemon. |
| `crates/typio-client` | — | Shared Rust client for the TIP Unix-socket protocol: framing, request/response calls, and event subscriptions. |
| `crates/typio-control` | `typioctl` | Command-line control client; maps resource+verb commands onto TIP RPCs. |
| `crates/typio-engine-protocol` | — | Typed wire contract for isolated engine processes, including the versioned fd 3 frame codec. |
| `crates/typio-engine-manifest` | — | Typed contract for engine manifests (`typio-engine-*.toml`). |
| `crates/typio-engine-check` | `typio-engine-check` | Black-box conformance and validation tool for manifest-declared engine processes. |
| `crates/typio-settings` | `typio-settings` | Graphical settings application. |
| `crates/xtask` | — | Cargo task helper: `cargo xtask install` and `cargo xtask uninstall`. |

## Crate: `typio-control`

The package is `typio-control`; the installed binary is `typioctl`. The crate is
a pure client: it links no C dependencies, never starts, forks, or execs the
daemon, and its only contract with the daemon is the TIP connection.

### Source Layout

| File | Role |
|---|---|
| `crates/typio-control/src/main.rs` | Argument parsing and dispatch. Declares the resource+verb command tree with `clap`, the global `-o`/`--output {plain\|json}` flag, and the `run` match that binds every verb to its handler. |
| `crates/typio-control/src/commands.rs` | Command handlers. Assemble TIP requests and render results as plain text or raw JSON, including the `engine use` kind resolution through `engine.list`. |

The crate holds no IPC code of its own. The UDS client is the shared
`typio-client` crate (`Client::connect` / `Client::call`), which supersedes the
crate's earlier local `src/ipc.rs`; `commands.rs` goes through that boundary
rather than speaking JSON-RPC directly.

## Where Implementation Coordinates Live

Documents under `docs/explanation/` are conceptual: they carry no source paths,
symbols, or code blocks (`[INV-CORE-04]`). Module and symbol coordinates live in
this page, in [Panel Appearance](panel-appearance.md), and in inline code
comments. Each subsystem that an explanation page describes gets a
`## Subsystem: <name>` section below, holding the source map, symbol ownership,
and file responsibilities that the explanation page deliberately omits.

## Subsystem: Candidate Panel Behavior

Coordinates for the user-visible candidate Panel lifecycle
([Candidate Panel Behavior](../explanation/candidate-panel-behavior.md)).

### Source Map

| Responsibility | Source |
|---|---|
| Composition callback → `(preedit, cursor_pos, candidates, selected)` slot | `crates/typio-daemon/src/keyboard/router.rs` |
| Inline preedit cursor resolution | `crates/typio-daemon/src/preedit.rs` |
| Host-managed-selection key interception (pure rules) | `crates/typio-daemon/src/candidate_guard.rs` |
| Owner arbitration + anchor generation + caret fallback | `crates/typio-host-types/src/panel_coordinator.rs` |
| Panel dirty schedule state | `crates/typio-host-types/src/panel_scheduler.rs` |
| Presentation de-duplication by `(generation, composition_seq)` | `crates/typio-host-types/src/panel_present_gate.rs` |
| Preedit/panel sync plan and positioned-UI timeout | `crates/typio-daemon/src/text_ui_state.rs` |
| Focus effects pipeline (clear_preedit, focus_out, reset) | `crates/typio-daemon/src/session_glue.rs`, `crates/typio-daemon/src/focus_controller.rs` |
| Candidate flush from the reactor step | `crates/typio-daemon/src/app/panel_driver.rs` |
| Offscreen flux render + SHM attach | `crates/typio-host-platform/src/panel.rs`, `crates/typio-host-platform/src/panel_shm.rs` |

The coordinator and the scheduler moved out of the daemon crate: the panel
schedule state machine today has exactly two states, `Idle` and `Dirty`
(`PanelScheduleState`), and has no `RETRY` variant — a dropped frame stays
`Dirty` and is retried from the update path.

### Host-Managed-Selection Vocabulary

The engine declares host-managed-selection capabilities on the composition it
publishes:

| Flag | Bit | Keys the host intercepts |
|---|---|---|
| `HostSelectionFlags::NAVIGATE` | `1 << 0` | Up / Down / Left / Right |
| `HostSelectionFlags::COMMIT` | `1 << 1` | Space |
| `HostSelectionFlags::INDEX_PICK` | `1 << 2` | `1`…`9`, `0` |
| `HostSelectionFlags::COMMIT_RAW` | `1 << 3` | Enter / KP_Enter |

The flag set is defined as `HostSelectionFlags` in
`crates/typio-host-types/src/lib.rs`; it rides the engine wire contract as the
`Composition.host_managed_selection` field (`crates/typio-engine-protocol/src/message.rs`,
mirrored in `crates/typio-runtime/src/core/engine/composition.rs` and
`crates/typio-daemon/src/keyboard/output.rs`), and the interception rules are in
`crates/typio-daemon/src/candidate_guard.rs`.

### Anchor-Probe Options

The lifecycle options in the explanation page are the `[display]` keys
`anchor_probe` and `anchor_probe_timeout_ms` (`platform.toml`; also editable in
the settings application):

| Value | Where it is set | Current value |
|---|---|---|
| Shipped default for the configured timeout | `crates/typio-settings/src/platform_config.rs` (`DEFAULT_ANCHOR_TIMEOUT_MS`) | 150 ms |
| Coordinator default when the configured value is below the floor | `crates/typio-host-types/src/panel_coordinator.rs` (`DEFAULT_ANCHOR_TIMEOUT_MS`) | 20 ms |
| Accepted range enforced by `PanelCoordinatorConfig::from_values` | `crates/typio-host-types/src/panel_coordinator.rs` (`MIN_ANCHOR_TIMEOUT_MS`, `MAX_ANCHOR_TIMEOUT_MS`) | 10–1000 ms |

## Subsystem: Control Surfaces

Coordinates for the TIP transport, its daemon-side dispatch, and the in-process
presentation surfaces
([Control Surfaces](../explanation/control-surfaces.md)).

### Source Map

| Responsibility | Source |
|---|---|
| Version, method and topic constants, socket-path resolution | `crates/typio-daemon/src/ipc/protocol.rs` |
| JSON-RPC envelope types and length-prefixed framing | `crates/typio-daemon/src/ipc/framing.rs` |
| epoll loop, peer credential check, frame and connection limits | `crates/typio-daemon/src/uds_server.rs` |
| Transport-agnostic dispatch (`StatusService` over a `ServiceBackend`) | `crates/typio-daemon/src/service.rs` |
| Runtime-backed backend (`TypioBackend`) and the notification emitter | `crates/typio-daemon/src/ipc_bus.rs` |
| State snapshot that both responses and tray refresh from | `crates/typio-daemon/src/state_controller.rs` |
| Shared TIP client used by every in-tree consumer | `crates/typio-client/src/lib.rs` |
| CLI command tree and handlers | `crates/typio-control/src/main.rs`, `crates/typio-control/src/commands.rs` |
| Tray menu model and SNI serialisation | `crates/typio-daemon/src/tray_menu.rs`, `crates/typio-daemon/src/tray_sni.rs` |

### Symbol Ownership

- `StatusService` performs the owned runtime operation over a `ServiceBackend`
  trait; the production implementation is `TypioBackend` in `ipc_bus.rs`, which
  borrows the main-loop-owned `TypioInstance` (`Rc<RefCell<_>>` runtime
  ownership stays on the loop thread).
- Tray callbacks enqueue `DaemonEvent::TrayAction`; `crates/typio-daemon/src/app/tray.rs`
  applies it on the main loop.
- Wire vocabulary: `hello` / `protocolVersion`, `events.subscribe`, the 1 MiB
  frame cap, 16 concurrent clients, and 16 subscribed topics per client. The
  method and topic catalog is the [IPC Protocol Reference](../reference/ipc-protocol.md).

## Subsystem: Event Loop Scheduling

Coordinates for the single-poll loop, its phase order, and its deadlines
([Event Loop Scheduling](../explanation/event-loop-scheduling.md)).

### Driver Map

| Driver | Responsibility |
|--------|----------------|
| `crates/typio-daemon/src/app/event_loop.rs` | I/O preparation and phase ordering |
| `crates/typio-daemon/src/app/reactor.rs` | Named fd sources, readiness snapshots, earliest-deadline timeout reduction (`PollSource`, `PollTimeout`) |
| `crates/typio-daemon/src/app/input_driver.rs` | Ordered key batches, engine output, virtual-keyboard forwarding, repeat |
| `crates/typio-daemon/src/keyboard/preedit_coalescer.rs` | Bounded latest-wins pure-preedit staging |
| `crates/typio-daemon/src/app/panel_driver.rs` | Panel schedule convergence, ownership, anchor, presentation retry |
| `crates/typio-daemon/src/config_watcher.rs` | Config-watch events, debounce timing, watch rearming, runtime reload boundary |
| `crates/typio-daemon/src/voice.rs` | Voice recording/inference state, deferred voice reload |

### Poll Timeout Participation

| Deadline | Source of the remaining time |
|---|---|
| Pure-preedit coalescing (2 ms quiet, 4 ms hard) | `KeyboardRouter::preedit_deadline_remaining_ms` |
| Positioned-UI anchor probe | `PanelCoordinator::anchor_deadline_remaining_ms` |
| Virtual-keyboard keymap wait | `wayland_pending.min_deadline_ms` in `crates/typio-host-types/src/wayland_pending.rs` |

Key repeat, the indicator timer, the voice-status timer, and the config-reload
debounce own a `timerfd` and contribute no timeout.

### Log-Emission Ownership

- focus-controller effect summaries: `crates/typio-daemon/src/app/event_loop.rs`
- teardown-cause and grab create/destroy logs: `crates/typio-daemon/src/session_glue.rs`, `crates/typio-daemon/src/focus_controller.rs`
- virtual-keyboard health and fail-safe logs: `crates/typio-host-platform/src/input_method.rs`
- per-key sequencing and modifier-path traces: `crates/typio-daemon/src/keyboard/router.rs`
- subscriber installation and level control: `crates/typio-daemon/src/diagnostics.rs`

The trace-capture pipeline (`typio --verbose 2>&1 | tee typio-trace.log`) and
the log-level mapping live in [Troubleshooting](../how-to/troubleshooting.md).

### Runtime-State Projection Fields

`RuntimeState` (`crates/typio-daemon/src/service.rs`) exports the projection the
explanation page describes by meaning:

| Field | Meaning |
|-------|---------|
| `lifecycle_phase` | `inactive` / `activating` / `active` / `deactivating` |
| `grab_state` | `absent` / `needs_keymap` / `ready` / `broken` |
| `active_key_generation` | current grab epoch |
| `keyboard_grab_active` | whether a grab object exists |
| `virtual_keyboard_state` | vk readiness |
| `virtual_keyboard_has_keymap` | vk has received a keymap |
| `virtual_keyboard_keymap_generation` | keymap epoch |
| `virtual_keyboard_drop_count` | cumulative dropped forwards |
| `virtual_keyboard_state_age_ms` | time since last vk state change |
| `virtual_keyboard_keymap_deadline_remaining_ms` | time until keymap timeout |

## Subsystem: Focus Controller

Coordinates for the derived-state focus/grab controller
([Focus Controller](../explanation/focus-controller.md)).

### Module Boundaries

| Module | Responsibility |
|--------|---------------|
| `crates/typio-daemon/src/focus_controller.rs` | `reduce`, `diff`, data structures. Pure, testable without frontend or Wayland. |
| `crates/typio-daemon/src/session_glue.rs` | `observe` (reads frontend fields) and `apply` (mutates frontend/Wayland state). Effectful, tied to `InputMethodFrontend`. |
| `crates/typio-daemon/src/app/mod.rs` (`FocusDriver::tick`) | Per-tick driver: records facts, calls reduce/observe/diff/apply in order. |
| `crates/typio-host-platform/src/input_method.rs` | Virtual-keyboard keymap/modifier handoff and readiness gating. The focus controller reads the `keymap_received_this_epoch` flag but does not own the transitions. |

### Symbols the Explanation Page Describes by Meaning

| Explanation phrase | Symbol |
|---|---|
| Wanted grab: hard teardown / soft pause / grab | `GrabWant` variants, derived by `reduce` into `DesiredState` |
| Actual grab resource: absent / awaiting keymap / ready | `GrabResourceState` (`GrabResourceState::Absent`, `NeedsKeymap`, `Ready`) |
| Fact fields | `im_activate_seen`, `im_deactivate_seen`, `im_done_had_activate`, `im_done_serial`, `connection_alive`, `suspend_gap_detected` |
| Effect names | `destroy_grab`, `reset_key_routing`, `discard_composition`, `clear_preedit`, `commit`, `create_grab`, `send_focus_in`, `send_focus_out`, `reactivate` (`EffectSet`) |
| Apply order | exactly the order above, executed by `session_glue::apply` |
| Soft-pause entry point | `keyboard_pause()` |
| Emergency keyboard reset teardown | `focus_hard_reset_keyboard` |
| Single update entry point | `FocusController::update` |

## Subsystem: Frontend Graphics

Coordinates for the CPU render and shared-memory present path
([Frontend Graphics](../explanation/frontend-graphics.md)).

### Render and Present Map

| Component | File | Responsibility |
|---|---|---|
| `FluxPanel` | `crates/typio-host-platform/src/panel.rs` | Layout cache, extent policy, CPU canvas lifecycle, `draw_candidates`, `draw_status_banner`, `present_shm`, `hide`, `reset_shm_pool`, `set_scale`, `set_font_config` |
| `TextRaster` | `crates/typio-host-platform/src/text_raster.rs` | One `flux_text*` context; `measure` and `draw` into the canvas via the host-coverage glyph path; family-class selection |
| `ShmBufferPool` / `ShmBuffer` | `crates/typio-host-platform/src/panel_shm.rs` | Fixed-capacity (`DEFAULT_CAP = 3`) pool, non-blocking `acquire`, per-buffer `BufferReleaseState`, `reset` |
| Popup surface | `crates/typio-host-platform/src/input_method.rs` | Creates and owns the input-popup `wl_surface` |

### Flux API Surface Actually Used

| Concept | Current use |
|---|---|
| `flux_canvas_create_cpu` / `flux_canvas_cpu_begin` / `end` | CPU canvas lifecycle — fills and rounded-rects into a host framebuffer. |
| `flux_canvas_cpu_pixels` | Direct pointer into the RGBA8 framebuffer (no bus, no fence). |
| `flux_canvas_fill_rrect` | Background and selection-highlight fills. |
| `flux_text_draw` | Shapes text and composites host-resident R8 glyph coverage into the CPU canvas. |

Do not reintroduce a Vulkan device, offscreen `flux_surface`, swapchain, dma-buf
present, GPU readback, or a GPU glyph atlas into this path: the retired
mechanisms and their reasons are tabulated in the
[Panel Rendering Blueprint](../architecture/panel-rendering.md).

### Extent Policy

`apply_surface_size` in `crates/typio-host-platform/src/panel.rs`,
`LayoutCacheKey` for the per-page layout cache:

| Rule | Value |
|---|---|
| Quantisation quantum | 64 px wide, 32 px high (with `wp_viewporter`) |
| Hysteresis shrink point | when required content needs no more than half of the retained axis |
| Hard cap | 16384 × 4096 logical pixels |

## Subsystem: Input-Method Session

Coordinates for the protocol session, the focus/grab resource, and text
transactions
([Wayland Input-Method v2 Session](../explanation/input-method-session.md)).

### Ownership Map

| Module | Responsibility |
|--------|---------------|
| `crates/typio-daemon/src/focus_controller.rs` | pure lifecycle decisions (`reduce`, `diff`) and the data structures that describe them |
| `crates/typio-daemon/src/session_glue.rs` | `observe` (live resource snapshot) and `apply` (effectful execution of the diff) |
| `crates/typio-daemon/src/keyboard_policy.rs` | pure modifier and repeat-cancellation predicates |
| `crates/typio-daemon/src/keyboard/router.rs` | the pure routing decision `(key, mods, state) → {action, reason}` |
| `crates/typio-host-platform/src/input_method.rs` (`Dispatch<ZwpInputMethodKeyboardGrabV2>`) | key-event interpretation (XKB → owned `DecodedKeyEvent`) while the session is focused |
| `crates/typio-host-platform/src/input_method.rs` (`forward_key`, `forward_modifiers`) | virtual-keyboard health, keymap/modifier handoff, readiness gating, and fail-safe downgrade |
| `crates/typio-daemon/src/app/mod.rs` (`App::run`) | poll scheduling, bounded auxiliary-fd dispatch, and deadline-aware wakeups |
| `crates/typio-daemon/src/config_watcher.rs` | config-watch events, debounce timing, watch rearming, and the runtime reload boundary |
| `crates/typio-daemon/src/voice.rs` | voice recording/inference state and deferred voice reload application |
| `xkb_state` | the logical modifier view |
| engine implementations | only engine/composition behavior |

The status D-Bus surface exports this state but does not own it.
`RuntimeState` is a read-only projection of `observe()`, not an independent
tracker.

### Session Symbols and Facts

| Explanation phrase | Symbol |
|---|---|
| Session editing facts | `SessionState` in `crates/typio-host-platform/src/input_method.rs` |
| Router-owned input context | `TypioInputContext`, owned by `KeyboardRouter` |
| Protocol commit point | `done(serial)`; serial tracking in `InputMethodState` |
| Grab want / readiness | `GrabWant`, `DesiredState`, `GrabResourceState` |
| Keymap-received flag | `keymap_received_this_epoch` |
| Keyboard ownership | `InputMethodState::keyboard_epoch`, `forwarded_keys`, `held_keys`; `KeyboardRouter::pressed_keys` |
| Soft-pause state shed at defocus | `physical_modifiers`, `saw_blocking_modifier`, the shortcut arbiter |
| Engine availability axis | `Uninitialized` / `Preparing` / `Ready` / `Failed` |
| Engine result contract | `InputContextResult::NOT_HANDLED` (any other result consumes the key) |
| Batch classification | `im_activate_seen` / `im_deactivate_seen` / `im_done_had_activate` / `im_done_had_deactivate` consumed by `reduce`; ADR-0018's pure classifier has no separate symbol |
| Text transaction entry point | `InputMethodState::text_transaction_and_flush`; lifecycle-only commits use `commit_protocol_state()` |
| Pending-key queue | `InputMethodState::pending_keys` |
| Language switch chord | `shortcuts.switch_language` in `crates/typio-runtime/src/config_schema.rs`, consumed by the arbiter in `crates/typio-daemon/src/keyboard/router.rs` |

## Subsystem: Stall Containment

Coordinates for the no-watchdog containment model and stall attribution
([Stall Containment](../explanation/stall-containment.md)).

### Latency Budgets

`request_timeout_for` in `crates/typio-runtime/src/core/engine/backend/process.rs`
selects the per-request budget:

| Operation | Budget | Constant |
|---|---|---|
| Hot-path keystroke (`process-key`) | 50 ms | `ENGINE_KEY_TIMEOUT` |
| `availability` query | 100 ms | `ENGINE_REQUEST_TIMEOUT` |
| Cold start (`init`) | 60 s | `ENGINE_INIT_TIMEOUT` |
| Handshake before HELLO | 5 s | `ENGINE_HANDSHAKE_TIMEOUT` |
| `reload-config` | 5 s | `ENGINE_RELOAD_TIMEOUT` |
| `invoke-command` / control plane | 5 s | `ENGINE_COMMAND_TIMEOUT` |
| Voice (`process-audio`) | 120 s | `ENGINE_VOICE_TIMEOUT` |
| Other engine requests | 500 ms | `ENGINE_DEFAULT_TIMEOUT` |
| Config file read | 2 s on a reader thread, at most 4 reads in flight | `CONFIG_READ_TIMEOUT`, `MAX_CONFIG_READS_IN_FLIGHT` in `crates/typio-runtime/src/config.rs` |

### Attribution Targets

`crates/typio-daemon/src/diagnostics.rs` installs the `tracing` subscriber whose
formatting layer writes to standard error (colourised only when stderr is a
terminal). Stall attribution reads the target namespaces it already emits:
`typio.wayland.io`, `typio.engine.key`, `typio.config`, `typio.lifecycle`, and
`typio.panel.*`. Do not reintroduce `set_stage`-style annotations, a stage enum,
or a baseline poll tick to advance a sampler
([ADR-0041](../adr/0041-remove-watchdog.md)).

## Subsystem: Runtime Architecture

Coordinates for the three runtime boundaries
([Runtime Architecture](../explanation/architecture-overview.md)).

### Boundary Map

| Boundary | Owner module |
|---|---|
| Wayland binding, protocol dispatch, surface lifetime | `crates/typio-host-platform/src/input_method.rs`, `crates/typio-host-platform/src/protocols.rs` |
| Poll loop, phase order, deadline folding | `crates/typio-daemon/src/app/event_loop.rs`, `crates/typio-daemon/src/app/reactor.rs` |
| Panel render and present | `crates/typio-host-platform/src/panel.rs`, `crates/typio-host-platform/src/panel_shm.rs` |
| PipeWire audio capture | `crates/typio-daemon/src/voice.rs` (`PwRecordSource`, the `pw-record` CLI) |
| Engine discovery | `crates/typio-daemon/src/engine_loader/mod.rs`, `crates/typio-daemon/src/engine_loader/dirs.rs` |
| Runtime state and registry | `crates/typio-runtime/src/instance.rs` (`TypioInstance`), `crates/typio-runtime/src/core/registry/mod.rs` |
| Engine process backend | `crates/typio-runtime/src/core/engine/backend/process.rs` (`ProcessBackend`) |
| Engine framing and payloads | `crates/typio-engine-protocol/src/frame.rs`, `crates/typio-engine-protocol/src/message.rs`, `crates/typio-engine-protocol/src/codec.rs` |
| Manifest contract | `crates/typio-engine-manifest/src/lib.rs` |
| Control socket, dispatch, backend | `crates/typio-daemon/src/uds_server.rs`, `crates/typio-daemon/src/service.rs`, `crates/typio-daemon/src/ipc_bus.rs` |
| Engine conformance | `crates/typio-engine-check/src/lib.rs`, `crates/typio-engine-check/src/session.rs` |

### Symbols the Explanation Page Describes by Meaning

| Explanation phrase | Symbol |
|---|---|
| The worker's opening handshake | `EngineHello` (`crates/typio-engine-protocol/src/message.rs`) |
| The activation handshake that supplies config, data, and state roots | `HostHello` |
| Engine process ownership: spawn, poison, respawn, shutdown | `ProcessBackend::launch_worker` |
| The runtime embedded in the daemon | `typio-runtime` built as an `rlib`, entered from `crates/typio-daemon/src/bin/typio.rs` |

Retired and not to be reintroduced: the C engine plugin ABI, `libtypio.so`, `dlopen` of engines, and every in-process engine adapter
([ADR-0046](../adr/0046-engine-protocol-only-runtime.md)).

## Subsystem: Composition State Machine

Coordinates for composition as state and commit as an ordered event
([Composition State Machine](../explanation/composition-state-machine.md)).

### Composition Types

| Concept | Symbol |
|---|---|
| Atomic preedit/candidate snapshot | `Composition` (`crates/typio-runtime/src/core/engine/composition.rs`), mirrored as the wire `Composition` in `crates/typio-engine-protocol/src/message.rs` |
| Preedit segment and its decoration | `PreeditSegment`, `PreeditFormat` |
| Candidate entry | `Candidate` |
| Snapshot fields | `segments`, `cursor_pos`, `candidates`, `page`, `page_size`, `total`, `selected`, `has_prev`, `has_next`, `host_managed_selection` |
| Ordered output queue | `ContextOutput::Commit` / `ContextOutput::Composition` / `ContextOutput::Clear` |
| Context that owns one composition | `TypioInputContext` (`crates/typio-runtime/src/input_context.rs`) |

### Host-Side Application

| Responsibility | Source |
|---|---|
| Composition callback slot fed from engine output | `crates/typio-daemon/src/keyboard/router.rs` |
| Inline preedit cursor resolution | `crates/typio-daemon/src/preedit.rs` |
| Applying commit and composition to the protocol | `crates/typio-host-platform/src/input_method.rs` |
| Candidate click request | wire `Request::CommitCandidate { context_id, index }` (`crates/typio-engine-protocol/src/message.rs`) |
| Focus and reset requests | wire `Request::FocusOut(context_id)`, `Request::Reset(context_id)` |
| Host-managed selection flags | `HostSelectionFlags` (`crates/typio-host-types/src/lib.rs`) |
| Interception of host-managed selection keys | `crates/typio-daemon/src/candidate_guard.rs` |

## Subsystem: Configuration System

Coordinates for the two persisted configuration files and their reload path
([Configuration System](../explanation/configuration-system.md)).

### Source Map

| Responsibility | Source |
|---|---|
| Owned config tree, dotted keys, bounded reader thread | `crates/typio-runtime/src/config.rs` (`Config`, `ConfigValue`, `ConfigError`, `CONFIG_READ_TIMEOUT`, `MAX_CONFIG_READS_IN_FLIGHT`) |
| TOML parsing and serialisation | `crates/typio-runtime/src/config/parse.rs`, `crates/typio-runtime/src/config/serialize.rs` |
| Built-in framework keys and defaults | `crates/typio-runtime/src/config_schema.rs` |
| Config directory creation and load path | `crates/typio-runtime/src/instance.rs` (`TypioInstance::init_rust`) |
| Engine schema registration and config-change delivery | `crates/typio-runtime/src/core/registry/mod.rs` |
| Configuration verbs over TIP | `crates/typio-daemon/src/ipc_bus.rs` |
| Config-file watch, relevance filter, debounce | `crates/typio-daemon/src/config_watcher.rs` (`RELEVANT_FILES`, `DEFAULT_DEBOUNCE`) |
| Host-side readers of the display keys | `crates/typio-daemon/src/app/indicator.rs`, `crates/typio-daemon/src/app/font_config.rs` |
| Graphical editor of the host-owned settings file | `crates/typio-settings/src/platform_config.rs` (`DEFAULT_THEME`, `DEFAULT_LAYOUT`, `DEFAULT_FONT_SIZE`, `DEFAULT_FONT_FAMILY`, `DEFAULT_ANCHOR_TIMEOUT_MS`) |

### Namespace Ownership

| Namespace | Registered by |
|---|---|
| Framework keys | `crates/typio-runtime/src/config_schema.rs` |
| Engine keys | the `SCHEMA` records carried in `EngineHello`, admitted only below `engines.<engine-name>.` by `crates/typio-runtime/src/core/registry/mod.rs` |

### Keys Read by the Host but Not Yet in the Reference

| Key | Read at | Default and clamp |
|---|---|---|
| `display.indicator_enabled` | `crates/typio-daemon/src/app/indicator.rs` | `true` |
| `display.indicator_duration_ms` | `crates/typio-daemon/src/app/indicator.rs` | 1500 ms, clamped 100–10000 ms |

Both belong in [Configuration Reference](../reference/configuration.md) under `[display]`.

## Subsystem: Engine Contract

Coordinates for the engine protocol's request and reply vocabulary
([Engine Contract](../explanation/engine-contract.md)).

### Request Catalog

`Request` (`crates/typio-engine-protocol/src/message.rs`):

| Request | Purpose |
|---|---|
| `Initialize`, `Shutdown`, `Deactivate` | Heavyweight start, process exit, optional-resource drop |
| `FocusIn(u64)`, `FocusOut(u64)`, `Reset(u64)` | Per-context focus and reset, keyed by context id |
| `ReloadConfig` | Refresh engine-namespaced settings |
| `ProcessKey(KeyEvent)` | One keyboard transaction |
| `Availability` | Current readiness axis |
| `ProcessAudio(Vec<u8>)` | One bounded audio block |
| `ListModes`, `GetActiveMode(u64)`, `SetActiveMode { .. }` | Mode catalog and switching |
| `CommitCandidate { context_id, index }` | A clicked candidate |
| `ListCommands`, `InvokeCommand(String)` | One-shot actions |

### Reply Records and Results

| Item | Symbol |
|---|---|
| Reply envelope | `Reply`, `ReplyRecord` (`Ok`, `Error`, `KeyResult`, `Availability`, `Text`, `Mode`, `Command`, `ActiveMode`, `Composition`, `Commit`, `Clear`) |
| Routing result (wire) | `KeyResult`: `NotHandled`, `Handled`, `PassThrough`; legacy aliases `COMPOSING` and `COMMITTED` decode to `Handled` (`crates/typio-engine-protocol/src/message.rs`) |
| Routing result (runtime mirror) | `KeyProcessResult`: `Handled`, `NotHandled`, `PassThrough` (`crates/typio-runtime/src/core/engine/event.rs`) |
| Readiness axis | `Availability`: `Uninitialized`, `Preparing`, `Ready`, `Failed` |
| Key description | `KeyEvent` (`KeyState` press/release, hardware code, keysym, base keysym, modifier mask, Unicode scalar, timestamp, repeat marker) |
| Mode record | `Mode` (id, labels, icon, profile), `ModeSalience`: `Quiet`, `Notable` |
| Command record | `Command` |
| Frame | magic `TYEP`, major 1, minor 0, 8 MiB payload cap (`crates/typio-engine-protocol/src/frame.rs`) |

### Process Model

| Responsibility | Source |
|---|---|
| Spawn, fd 3 duplication, `TYPIO_ENGINE_PROTOCOL` / `TYPIO_ENGINE_FD`, `ETXTBSY` retries | `crates/typio-runtime/src/core/engine/backend/process.rs` (`launch_worker`) |
| Handshake, schema probe, poisoning, asynchronous respawn, backoff | `crates/typio-runtime/src/core/engine/backend/process.rs` |
| Per-request deadlines | `request_timeout_for` in `crates/typio-runtime/src/core/engine/backend/process.rs` |

## Subsystem: Engine-to-Host Resource Flow

Coordinates for which channel carries which resource
([Engine-to-Host Resource Flow](../explanation/engine-host-resource-flow.md)).

### Resource Channel Map

| Resource | Wire carrier | Owning types |
|---|---|---|
| Manifest metadata | `typio-engine-*.toml` parsed by `crates/typio-engine-manifest/src/lib.rs` | `crates/typio-daemon/src/engine_loader/manifest.rs` |
| Configuration schema | `EngineHello` `SCHEMA` records | `crates/typio-runtime/src/core/registry/mod.rs` |
| Mode | `ReplyRecord::Mode` / `ActiveMode` | `Mode`, `ModeSalience` in `crates/typio-runtime/src/core/engine/mode.rs` |
| Composition, commit, clear | `ReplyRecord::Composition` / `Commit` / `Clear` | `Composition`, `ContextOutput` in `crates/typio-runtime/src/core/engine/composition.rs` |
| Voice text | `ReplyRecord::Text` | `crates/typio-runtime/src/voice/types.rs` |
| Availability | `ReplyRecord::Availability` | `Availability` in `crates/typio-runtime/src/core/engine/event.rs` |
| Commands | `ReplyRecord::Command` | `Command`, `crates/typio-runtime/src/core/registry/mod.rs` |

Every decode is typed and bounded: frames are capped in
`crates/typio-engine-protocol/src/frame.rs` and the payload is decoded in
`crates/typio-engine-protocol/src/codec.rs`. A poisoned channel is never reused
(`crates/typio-runtime/src/core/engine/backend/process.rs`).

## Subsystem: Modifier-Key Consumption

Coordinates for the key-consumption rule
([Modifier-Key Consumption](../explanation/modifier-key-consumption.md)).

### Consumption Rule

| Element | Coordinate |
|---|---|
| Host decision point | `TypioInputContext::process_key` (`crates/typio-runtime/src/input_context.rs`) returns `result != KeyProcessResult::NotHandled`, so every other result consumes the key |
| Runtime result enum | `KeyProcessResult` (`crates/typio-runtime/src/core/engine/event.rs`); the `PassThrough` variant's doc comment still describes forwarding to the application |
| Wire result enum | `KeyResult` (`crates/typio-engine-protocol/src/message.rs`) |
| Modifier mask derivation | `crates/typio-host-types/src/modifiers.rs`, `crates/typio-daemon/src/keyboard_policy.rs`, `crates/typio-host-platform/src/input_method.rs` (`forward_modifiers`) |
| Key description carried to the engine | `KeyEvent` (`crates/typio-engine-protocol/src/message.rs`) |

Open divergence: `PassThrough` is consumed exactly like `Handled` today. Closing
it means teaching `process_key` to distinguish the two, not changing the
protocol enum.

## Subsystem: Panel Architecture

Coordinates for the Panel's policy layer and its render layer
([Panel Architecture](../explanation/panel-architecture.md)).

### Ownership and Policy

| Responsibility | Source |
|---|---|
| Visible-owner arbitration, anchor generations, probe bookkeeping | `crates/typio-host-types/src/panel_coordinator.rs` (`PanelCoordinator`, `PanelCoordinatorConfig`) |
| Schedule state | `crates/typio-host-types/src/panel_scheduler.rs` (`PanelScheduleState::Idle` / `Dirty`) |
| Presentation de-duplication | `crates/typio-host-types/src/panel_present_gate.rs` (`PresentationRecord`) |
| Panel convergence in the reactor step | `crates/typio-daemon/src/app/panel_driver.rs` (`flush_candidate_panel`, `hide_candidate_panel`) |
| Indicator and voice-status producers | `crates/typio-daemon/src/app/indicator.rs` |
| Anchor deadline in the poll timeout | `PanelCoordinator::anchor_deadline_remaining_ms` |

### Render and Present

| Responsibility | Source |
|---|---|
| Layout cache, extent policy, CPU canvas, `draw_candidates`, `draw_status_banner`, `present_shm` | `crates/typio-host-platform/src/panel.rs` (`FluxPanel`) |
| Shared-memory pool and per-buffer release state | `crates/typio-host-platform/src/panel_shm.rs` (`ShmBufferPool`, `ShmBuffer`, `BufferReleaseState`) |
| Text measurement and rasterisation | `crates/typio-host-platform/src/text_raster.rs` (`TextRaster`) |
| Popup surface creation and ownership | `crates/typio-host-platform/src/input_method.rs` |

### Corrections Applied to the Explanation Page

- The Panel Coordinator and the panel scheduler live in `typio-host-types`, not in the daemon crate; they moved with the panel split.
- The schedule state machine has exactly two states, `Idle` and `Dirty`. There is no `RETRY` state: a frame the compositor has not released stays `Dirty` and is retried from the update path ([ADR-0022](../adr/0022-panel-retry-result-owned-by-update.md)).
- The GPU glyph atlas, the Vulkan offscreen render, and the dma-buf present path are retired; the only present mechanism is the CPU canvas plus `wl_shm` ([ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md)).
- The anchor-probe switch and timeout values, and the coordinator's own defaults and clamping range, are tabulated under [Anchor-Probe Options](#anchor-probe-options) above.

## Subsystem: Performance and Idle Power

Coordinates for the idle loop, its deadlines, and the bounded-work budgets
([Performance & Idle-Power Strategy](../explanation/performance-strategy.md)).

### Poll Set and Timers

| Responsibility | Source |
|---|---|
| Named poll sources and the readiness snapshot | `crates/typio-daemon/src/app/reactor.rs` (`PollSource`) |
| Unbounded baseline timeout and earliest-deadline reduction | `crates/typio-daemon/src/app/reactor.rs` (`PollTimeout`) |
| Phase order of one reactor step | `crates/typio-daemon/src/app/event_loop.rs` |
| Key-repeat timer | `crates/typio-daemon/src/repeat_timer.rs` |
| Config-reload debounce and rearm | `crates/typio-daemon/src/config_watcher.rs` (`DEFAULT_DEBOUNCE` = 100 ms) |
| Indicator and voice-status auto-hide timers | armed through `crates/typio-daemon/src/app/one_shot_timer.rs` |

### Deadline Sources

| Deadline | Symbol |
|---|---|
| Pure-preedit coalescing (2 ms quiet, 4 ms hard) | `KeyboardRouter::preedit_deadline_remaining_ms` (`crates/typio-daemon/src/keyboard/router.rs`), `PreeditCoalescer` (`crates/typio-daemon/src/keyboard/preedit_coalescer.rs`) |
| Positioned-UI anchor probe | `PanelCoordinator::anchor_deadline_remaining_ms` (`crates/typio-host-types/src/panel_coordinator.rs`) |
| Wayland response diagnostics | `wayland_pending.min_deadline_ms` (`crates/typio-host-types/src/wayland_pending.rs`) |
| Immediate re-dispatch | the reactor's zero-length timeout reduction |

### Bounded Work

| Stage | Bound |
|---|---|
| Engine keystroke IPC | 50 ms (`ENGINE_KEY_TIMEOUT`) |
| Engine availability query | 100 ms (`ENGINE_REQUEST_TIMEOUT`) |
| Config file read | 2 s on a reader thread, at most four in flight (`CONFIG_READ_TIMEOUT`, `MAX_CONFIG_READS_IN_FLIGHT` in `crates/typio-runtime/src/config.rs`) |
| Panel acquire and present | non-blocking `wl_shm` acquire in `crates/typio-host-platform/src/panel.rs` |

Build profiles are not a runtime coordinate: the contributor build uses Meson's
`debugoptimized` profile for the flux family and Cargo for Rust, and shipping
builds use `release`. See [Developer Setup](setup.md). Measurement tooling is
`powertop`, whose per-process wakeups-per-second row is the metric — it is not
otherwise documented in this repository.

## Subsystem: Project Scope

Coordinates for the workspace's dependency edges and retired names
([Project Scope](../explanation/project-scope.md)).

### Dependency Direction

| From | To | Mechanism |
|---|---|---|
| engine executable | `crates/typio-runtime` | Typio Engine Protocol on fd 3 (`crates/typio-engine-protocol/src/frame.rs`) |
| `crates/typio-runtime` | `crates/typio-daemon` | built as an `rlib` and entered from `crates/typio-daemon/src/bin/typio.rs` |
| `crates/typio-control`, `crates/typio-settings` | `crates/typio-client` | `Client::connect` / `Client::call` (`crates/typio-client/src/lib.rs`) |
| `crates/typio-client` | `crates/typio-daemon` | TIP over a filesystem UDS (`crates/typio-daemon/src/uds_server.rs`) |
| `crates/typio-engine-check` | engine executable | manifest plus `typio-engine-protocol` (`crates/typio-engine-check/src/session.rs`) |

### Retired Names

| Retired | Superseded by |
|---|---|
| C engine plugin ABI, vtables, factories, callback user data | Typio Engine Protocol ([ADR-0046](../adr/0046-engine-protocol-only-runtime.md)) |
| `libtypio.so`, installed engine headers, pkg-config metadata | direct worker executables |
| `dlopen` of engines and in-process engine adapters | `ProcessBackend` (`crates/typio-runtime/src/core/engine/backend/process.rs`) |
| The generic `typio-engine-worker` bridge and the `.engine` / `worker-v2` / `typio-engine-ipc` manifest spellings | `typio-engine-*.toml` manifests |

## Subsystem: Security Model

Coordinates for the enforcement points behind the trust-boundary claims
([Security Model](../explanation/security-model.md)).

### Enforcement Points

| Surface | Source |
|---|---|
| Socket mode `0600`, `SO_PEERCRED` uid check, frame and connection limits | `crates/typio-daemon/src/uds_server.rs` |
| 4-byte big-endian length prefix, 1 MiB frame cap, `serde_json` decode | `crates/typio-daemon/src/ipc/framing.rs` |
| Engine search path: compile-time system directory, `--engine-dir`, `$TYPIO_ENGINE_PATH` | `crates/typio-daemon/src/engine_loader/dirs.rs` |
| Explicit absolute manifest path through `engine.load` | `crates/typio-daemon/src/ipc_bus.rs` (`TypioBackend::engine_load`) |
| Worker spawn with stdio reserved for logs and stdin closed | `crates/typio-runtime/src/core/engine/backend/process.rs` |
| Input-method role grant | the compositor; the daemon binds it in `crates/typio-host-platform/src/input_method.rs` |

### Sandbox Roadmap (Not Implemented)

`launch_worker` in `crates/typio-runtime/src/core/engine/backend/process.rs` is
the single spawn site, so per-engine confinement — Landlock, seccomp, or
service-manager properties such as `NoNewPrivileges=` and
`RestrictNamespaces=` — would be added there. Nothing enforces confinement
today, which is why installing a manifest is equivalent to installing a
keylogger. There is no fuzz target in the tree yet; the TIP JSON decoder in
`crates/typio-daemon/src/ipc/framing.rs` is the component that parses
externally supplied bytes.

## Subsystem: Voice Input

Coordinates for the capture, inference, and delivery split
([Voice Input](../explanation/voice-input.md)).

| Responsibility | Source |
|---|---|
| PipeWire capture through the `pw-record` CLI | `crates/typio-daemon/src/voice.rs` (`PwRecordSource`) |
| Voice session, owned audio sink, event drain | `crates/typio-runtime/src/voice/session.rs` (`VoiceSession`, `AudioSink`, `AudioSource`) |
| Sample preparation (bounded 16 kHz mono) | `crates/typio-runtime/src/voice/audio.rs` |
| Voice types and outcomes | `crates/typio-runtime/src/voice/types.rs`, `crates/typio-daemon/src/voice.rs` (`VoiceController`, `VoiceOutcome`) |
| Session dispatch from the loop | `crates/typio-daemon/src/app/event_loop.rs` |
| In-flight worker transport handle | `crates/typio-runtime/src/core/engine/backend/process.rs` (`VoiceProcessHandle`) |
| Voice request budget (120 s) | `ENGINE_VOICE_TIMEOUT` in `crates/typio-runtime/src/core/engine/backend/process.rs` |

## Subsystem: Wayland Input Method Protocol

Coordinates for the protocol binding, the serial chokepoint, and the staging
rules ([Wayland Input Method Protocol](../explanation/wayland-input-method.md)).

### Protocol Bindings

| Responsibility | Source |
|---|---|
| Protocol XML definitions | `protocols/input-method-unstable-v2.xml`, `protocols/virtual-keyboard-unstable-v1.xml`, `protocols/text-input-unstable-v3.xml`, `protocols/viewporter.xml`, `protocols/fractional-scale-v1.xml` |
| Generated bindings per protocol | `crates/typio-host-platform/src/protocols.rs` (generated at compile time from the XMLs, no `wayland-protocols` crate) |
| Focus, text, grab, popup, and virtual-keyboard handlers | `crates/typio-host-platform/src/input_method.rs` (`Dispatch<ZwpInputMethodV2>`, `Dispatch<ZwpInputMethodKeyboardGrabV2>`) |
| Keymap handoff, modifier mirror, key forwarding | `crates/typio-host-platform/src/input_method.rs` (`forward_key`, `forward_modifiers`) |

### Session and Transaction Symbols

| Explanation phrase | Symbol |
|---|---|
| Session editing facts and the pending fact buffer | `SessionState`, `InputMethodState` (`crates/typio-host-platform/src/input_method.rs`) |
| Commit serial and the serial-0 write barrier | serial tracking in `InputMethodState`; `InputMethodState::text_transaction_and_flush(commit_text, preedit)` for text payloads and `InputMethodState::commit_protocol_state()` for lifecycle-only commits |
| Focus classification at the protocol commit boundary | `reduce` in `crates/typio-daemon/src/focus_controller.rs`; ADR-0018's pure `classify_done` helper has no symbol in the tree |
| Preedit update plan | `text_ui_plan_update`, `TextUiPlan::SyncPreeditAndPanel` / `TextUiPlan::SyncPanelOnly` (`crates/typio-daemon/src/text_ui_state.rs`) |
| Preedit coalescing (2 ms quiet, 4 ms hard) | `crates/typio-daemon/src/keyboard/preedit_coalescer.rs` (`PreeditCoalescer`, `PreeditUpdate`) |
| Keyboard epoch fence, ordered events, and paired virtual-keyboard output | `crates/typio-host-platform/src/input_method.rs` (`KeyboardInput`, `prepare_input`, `forward_key`) |
| Resume detector (logind sleep signal plus boot-time gap heuristic) | `crates/typio-daemon/src/resume_signal.rs` (`ResumeSignal`, `ResumeEvent`) |
| Indicator show paths and their gates | `IndicatorPath::Focus` / `Reactivate` / `StateChange` (`crates/typio-daemon/src/app/indicator.rs`); `Indicator::show_on_focus` / `show_on_reactivate` / `show_for_state_change` (`crates/typio-daemon/src/indicator.rs`) |
| Mode salience gate | `ModeSalience::Notable` / `Quiet` (`crates/typio-runtime/src/core/engine/mode.rs`) |
| Indicator auto-hide duration | `display.indicator_duration_ms` (read in `crates/typio-daemon/src/app/indicator.rs`) — not yet published in [Configuration Reference](../reference/configuration.md) |

## See Also

- [Panel Appearance](panel-appearance.md)
- [Developer Setup](setup.md)
- [Testing](testing.md)
