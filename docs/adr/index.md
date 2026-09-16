# Architecture Decision Records

ADRs are append-only records of significant design decisions in the Typio
workspace. Once accepted, a record is never edited: a decision changes by
authoring a new ADR that supersedes it ([INV-ARCH-01](../governance/documentation/core/invariants.md)).

Every entry below carries a short decision summary and the invariant it still
enforces, so that a reader — or an agent — can decide whether a record applies
before loading it. Retired records stay in the registry with cold-storage
pointers rather than disappearing ([INV-TEMP-03](../governance/documentation/core/invariants.md));
history lives under [`archive/`](archive/index.md).

Related material:

- How the system works *today*: [Architecture Blueprints](../architecture/index.md).
- The record template: [template.md](template.md).
- Rule for adding a record: the three-question significance test in the
  [ADR entity](../governance/documentation/profiles/architecture/adr.md).

> **Candidate-popup lag — reading guide for ADR-0006 / 0010–0013.** The popup's
> candidate-switch lag was diagnosed in four passes, and the first three each
> *mis-attributed* the root cause. The actual cause was a **per-page swapchain
> rebuild** (ADR-0013). The earlier ADRs remain valid as **independent**
> improvements — non-blocking present (0010), colour-independent coverage glyphs
> (0011), shared glyph atlas (0012) — each fixes a real problem and is still in
> force; only their "this is what cured the lag" claim was wrong, and each now
> carries a scope-correction block. The misdiagnosis trail is kept deliberately:
> it is the record that stops the next reader repeating it.
> Later retry-latch and scheduler regressions in ADR-0015's deferral path are
> recorded in ADR-0022 and ADR-0023. Do not implement new scheduling from the
> historical `panel_update_pending` or retry-latch descriptions in ADR-0015 /
> ADR-0022; the current implementation has an `IDLE` / `DIRTY` scheduler and
> SHM-buffer back-pressure. ADR-0040 superseded ADR-0036's
> `wl_surface.frame` soft gate.

## Registry

Superseded records stay in the table so that citations resolve; their
authority is listed under [Retired records](#retired-records).

| ID | Title | Status | Scope | Decision Summary & Primary Invariant | Date | Living Snapshot |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| [0001](0001-record-architecture-decisions.md) | Record Architecture Decisions | Accepted | process | ADRs are numbered, append-only records in `docs/adr/`; a changed decision is superseded, never rewritten. Invariant: accepted records are immutable. | 2026-05-28 | - |
| [0002](0002-wayland-input-method-v2.md) | Adopt `zwp_input_method_v2` as the host protocol | Accepted | session | Bind `zwp_input_method_v2` at runtime and treat its unstable status as managed risk. Invariant: the engine layer knows nothing about Wayland; one module speaks the protocol. | 2026-05-28 | [Input Session](../architecture/input-session.md) |
| [0003](0003-session-controller-reduce-diff.md) | Session controller — derived state, idempotent diff | Accepted | session | Lifecycle state is derived per step (`reduce` → `observe` → `diff`) instead of stored in a phase machine. Invariant: no stored phase; every effect is an idempotent diff against observed resources. | 2026-05-28 | [Input Session](../architecture/input-session.md) |
| [0004](0004-event-loop-scheduling-and-watchdog.md) | Event-loop scheduling and watchdog | Superseded by ADR-0041 (watchdog removed; loop scheduling retained) | loop | One single-threaded poll loop multiplexes every source, with bounded auxiliary work per tick and one coalesced render flush per iteration. Invariant: the loop is single-threaded and renders are deferred, not inlined. | 2026-05-28 | [Daemon Lifecycle](../architecture/daemon-lifecycle.md) |
| [0005](0005-unified-panel-backend.md) | Unified panel backend for candidate and status UI | Accepted | panel | One multi-zone Panel backend serves candidates and status UI over a platform-free content model. Invariant: the content model carries no Wayland or GPU types; only one input-popup surface exists. | 2026-05-28 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0006](0006-resilient-candidate-popup-present.md) | Resilient candidate-popup GPU present | Accepted (amended by ADR-0010; GPU present later removed by ADR-0040) | panel | Bound the popup's present operation with a timeout, skip-and-retry, and a recovery streak rather than blocking the loop. Invariant: a stalled compositor must never block the event loop. | 2026-05-28 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0007](0007-dbus-adapter-over-status-service.md) | D-Bus adapter as a thin transport over `TypioStatusService` | Superseded by ADR-0008 | control | Make each transport a thin adapter over one service, so business logic lives in exactly one place. Invariant (survives supersession): one source of truth per control operation. | 2026-05-28 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0008](0008-ipc-protocol-resource-namespaces-uds-only.md) | TIP v1 — IPC Protocol with resource namespaces, UDS-only, push events | Accepted (engine verbs amended by ADR-0026) | control | One resource-oriented, camelCase JSON-RPC control protocol over the UDS only, with no shims and no deprecation aliases. Invariant: a single control transport with dotted namespaces. | 2026-05-29 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0009](0009-long-term-performance-optimizations.md) | Long-term performance optimizations — font cache purging, composition short-circuit, and snapshot fast-path | Accepted | performance | Keep font and composition caches bounded, purge them on config reload, and skip work when content is unchanged. Invariant: caches are fixed-size and reclaimed deterministically. | 2026-05-29 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0010](0010-non-blocking-candidate-popup-present.md) | Non-blocking present mode for the candidate popup | Superseded by ADR-0040 (WSI present removed) | panel | Create the popup's present path without vsync so steady-state presentation stops blocking. Invariant (survives): the diagnosis that present must never block key handling. | 2026-05-29 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0011](0011-colour-independent-coverage-glyphs.md) | Colour-independent coverage glyph textures (draw-time tint) | Accepted (texture model superseded by ADR-0012) | panel | Rasterise glyphs as single-channel coverage and apply colour as a draw-time tint, so one texture serves every colour. Invariant: colour is never baked into or keyed into a glyph texture. | 2026-05-29 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0012](0012-glyph-atlas-shared-texture.md) | Shared glyph atlas (rasterise once, reference sub-rects) | Accepted (reclamation reworked by ADR-0020; GPU atlas retired by ADR-0040) | panel | Rasterise each glyph once into a shared atlas and draw tinted sub-rect quads. Invariant: a warmed cache performs no new uploads. | 2026-05-29 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0013](0013-grow-only-popup-swapchain.md) | Grow-only popup swapchain (stop rebuilding per candidate page) | Superseded by ADR-0044 (bounded retention replaces indefinite growth) | panel | Quantise the popup buffer and grow only, cropping to content with a viewport, so paging never rebuilds the surface. Invariant (survives): no per-page surface rebuild. | 2026-05-29 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0014](0014-canonical-panel-vocabulary.md) | Canonical panel vocabulary and module ontology | Accepted (refines ADR-0005) | panel | Adopt one word per concept — Panel as the multi-zone umbrella, `popup` only for the protocol surface — with a pure tier free of platform types. Invariant: vocabulary collisions are design defects. | 2026-05-30 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0015](0015-candidate-popup-lag-final-fixes.md) | Candidate popup lag — final fixes (acquire timeout, retry deferral, persistent upload context) | Accepted (retry design superseded by ADR-0022/0023; do not implement scheduling from it) | performance | Bound the acquire timeout sharply, defer the flush while a retry is pending, and reuse an upload context. Invariant: a present retry must never block key handling. | 2026-05-30 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0016](0016-per-glyph-font-fallback.md) | Per-glyph font fallback with format-12 charmap selection | Accepted (arbitrary-family reading superseded by ADR-0050) | panel | Fall back per glyph, verified against the font's charmap, and load fonts with the charmap that resolves supplementary-plane glyphs. Invariant: fallback is per glyph and verified, not assumed. | 2026-05-31 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0017](0017-positioned-ui-arbitration.md) | Positioned UI arbitration for panel owners | Accepted | panel | Producers submit requests with an explicit UI owner; exactly one owner is visible, later replaces earlier, and placement waits for a trusted anchor. Invariant: one visible owner, and positioned UI has a bounded anchor wait. | 2026-06-01 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0018](0018-focus-transition-classification.md) | Focus-transition classification and re-activation | Accepted | session | Classify focus transitions with one pure function and re-anchor the panel on re-activation while keeping the grab and engine context. Invariant: re-activation never rebuilds the grab. | 2026-06-01 | [Input Session](../architecture/input-session.md) |
| [0019](0019-atlas-hash-compaction.md) | Atlas hash-table compaction for sustained CJK input | Superseded by ADR-0020 | panel | Compact the glyph hash table at high load instead of letting dead entries accumulate. Invariant (superseded): hash-only compaction never reclaimed texture space. | 2026-06-02 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0020](0020-atlas-reclamation-and-glyph-layer-modularization.md) | Atlas texture reclamation and glyph-layer modularization | Accepted (supersedes ADR-0019) | panel | Reclaim the glyph store by full rebuild, and give every cache the same bound/evict/reclaim/observe contract. Invariant: reclamation is bounded by page size, not by session history. | 2026-06-02 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0021](0021-systemd-user-service-daemon-lifecycle.md) | systemd user service for daemon lifecycle | Accepted | loop | Ship the systemd user service as the only packaged startup surface, with journal logging, and drop desktop/autostart entries. Invariant: no second startup path. | 2026-06-03 | [Daemon Lifecycle](../architecture/daemon-lifecycle.md) |
| [0022](0022-panel-retry-result-owned-by-update.md) | Panel retry result owned by update | Accepted (amends ADR-0015) | panel | A present retry is the result of one update, not durable surface state, so a single retry cannot permanently suppress rendering. Invariant: retry ownership is per-update. | 2026-06-03 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0023](0023-panel-scheduler-state-machine.md) | Panel Scheduler State Machine | Accepted (amends ADR-0022) | panel | Replace the pending boolean with an explicit schedule state; only the event loop flushes, and key routing merely marks work dirty. Invariant: key routing never renders. | 2026-06-03 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0024](0024-idle-driven-loop-and-demand-gated-watchdog.md) | Idle-driven event loop and demand-gated watchdog | Superseded by ADR-0041 (watchdog removed; idle-driven loop retained) | loop | Block indefinitely when idle and fold every deadline into the poll timeout, so an idle daemon performs zero wakeups. Invariant: every time-based wake is an explicit deadline. | 2026-06-05 | [Daemon Lifecycle](../architecture/daemon-lifecycle.md) |
| [0025](0025-engine-discovery-search-path.md) | Engine discovery — ordered search path, no user-level auto-scan | Accepted | engine | Discover engines through an ordered search path: explicit flags, then an environment path, then the package-managed system directory. Invariant: no user-writable directory is auto-scanned. | 2026-06-05 | [Engine Runtime](../architecture/engine-runtime.md) |
| [0026](0026-modality-explicit-engine-control-surface.md) | Modality-explicit engine control surface (`keyboard.*` / `voice.*`) | Accepted (amends ADR-0008) | control | Name the modality in every verb that acts on an engine, leaving `engine.*` as aggregates only. Invariant: an ambiguous "active engine" verb cannot be reintroduced. | 2026-06-05 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0027](0027-ipc-engine-manifests.md) | IPC Engine Manifests | Superseded by ADR-0030 | engine | Discover engines by manifest rather than by library filename, so engines are never loaded into the daemon. Invariant (superseded): engine definition is metadata, not a path pattern. | 2026-06-05 | [Engine Runtime](../architecture/engine-runtime.md) |
| [0028](0028-direct-ipc-engine-workers.md) | Direct IPC Engine Workers | Superseded by ADR-0030 | engine | Each engine package owns a direct worker executable instead of sharing a generic bridge. Invariant (survives): no shared-loader indirection between host and engine. | 2026-06-06 | [Engine Runtime](../architecture/engine-runtime.md) |
| [0029](0029-engine-package-install-layout.md) | Engine Package Install Layout | Accepted (terminology amended by ADR-0030) | engine | Split the install layout by role: worker executables in libexec, arch-independent manifests in datadir, icons in the icon theme. Invariant: executables never sit in a data directory. | 2026-06-06 | [Engine Runtime](../architecture/engine-runtime.md) |
| [0030](0030-engine-process-manifests.md) | Engine process manifests and Typio Engine Protocol | Accepted (manifest keys extended by ADR-0031; amended by ADR-0046) | engine | Load only manifests that declare the engine protocol, and carry that protocol on a private descriptor with standard streams reserved for logs. Invariant: the protocol never rides stdin/stdout. | 2026-06-08 | [Engine Runtime](../architecture/engine-runtime.md) |
| [0031](0031-language-first-switching-surface.md) | Language-first switching surface (`language.*`, `languages` manifest key) | Accepted (amends ADR-0008/0026; TIP v3) | control | Make the language the user-facing switch unit across manifest, shortcut, control namespace, event, and status. Invariant: switching retargets keyboard and voice together. | 2026-06-12 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0032](0032-tray-icon-composition.md) | Tray icon composition — language base + modality overlays | Accepted (icon precedence chain amended by ADR-0033) | control | Compose the tray icon from layered channels: language identity as the base, voice presence on the overlay. Invariant: voice never competes for the base icon. | 2026-06-13 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0033](0033-language-led-tray-surface.md) | Language-led tray surface (icon and menu) | Accepted (amends ADR-0031 menu and ADR-0032 icon chain) | control | Drive both tray dimensions from the active language: one resolver chain for the icon, engines nested under language submenus. Invariant: engine identity never appears on the base icon. | 2026-06-17 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0034](0034-dynamic-engine-capabilities.md) | Dynamic engine capabilities (runtime-mutable language declarations) | Accepted (amends ADR-0031; revises ADR-0033) | control | Treat the manifest language list as a conservative floor that a runtime self-report replaces. Invariant: the host never assumes a static capability set. | 2026-06-17 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0035](0035-bilingual-migration-to-rust.md) | Bilingual migration of the host to Rust | Accepted (D1 superseded by ADR-0038; D5 superseded for settings by ADR-0045) | process | Port the C host to Rust leaf-to-root with C and Rust coexisting, verified per subsystem against a live compositor. Invariant: no big-bang rewrite; local protocol XML is the single source of protocol truth. | 2026-06-21 | [Workspace Topology](../architecture/workspace-topology.md) |
| [0036](0036-soft-present-gate-for-candidate-panel.md) | Soft Present Gate for the Candidate Panel | Superseded by ADR-0040 (gate survives only as pacing hygiene) | panel | Treat the compositor's frame callback as a soft limit rather than a hard lock, and present the latest coalesced state when it expires. Invariant (superseded): presentation pacing must not gate a frame indefinitely. | 2026-06-30 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0037](0037-demand-armed-watchdog-cadence.md) | Demand-Armed Watchdog Cadence | Superseded by ADR-0041 (watchdog removed) | loop | Start the watchdog disarmed and arm it on activation, so an idle session is not sampled. Invariant (superseded): sampling follows demand. | 2026-06-30 | [Daemon Lifecycle](../architecture/daemon-lifecycle.md) |
| [0038](0038-framework-abi-vet-monorepo.md) | Framework, ABI, and Vet Monorepo | Accepted (ABI workspace decision superseded by ADR-0046) | process | Land framework, ABI, conformance tooling, and host in one workspace with shared path dependencies so boundary changes land atomically. Invariant: cross-cutting changes are one commit. | 2026-06-30 | [Workspace Topology](../architecture/workspace-topology.md) |
| [0039](0039-cli-workspace-integration.md) | CLI Workspace Integration | Accepted (client duplication and settings deferral superseded by ADR-0045) | control | Bring the control CLI into the workspace so daemon and client protocol changes land together. Invariant: one workspace owns both ends of the control protocol. | 2026-06-30 | [Workspace Topology](../architecture/workspace-topology.md) |
| [0040](0040-cpu-canvas-render-shm-buffers.md) | CPU Canvas Render with Host-Managed SHM Buffers | Accepted (supersedes ADR-0010/0013/0036; grow-only note superseded by ADR-0044) | panel | Render the Panel on the CPU and present over shared memory only, removing the GPU device, readback, and dmabuf paths. Invariant: shared memory is the only Panel present path. | 2026-01-12 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0041](0041-remove-watchdog.md) | Remove the host watchdog | Accepted (supersedes ADR-0004/0024/0037) | loop | Delete the watchdog and bound the one genuinely blocking operation at its call site instead. Invariant: every main-loop stage is non-blocking or explicitly bounded; there is no runtime self-heal. | 2026-07-08 | [Daemon Lifecycle](../architecture/daemon-lifecycle.md) |
| [0042](0042-text-input-transaction-staging.md) | Text-input transaction staging at key-batch boundaries | Accepted (amended by ADR-0043) | session | Stage text updates as explicit transactions at key-batch boundaries and flush them through one entry point. Invariant: only the protocol module sends text; nothing else commits raw state. | 2026-07-09 | [Input Session](../architecture/input-session.md) |
| [0043](0043-bounded-preedit-coalescing.md) | Bounded preedit coalescing across reactor steps | Accepted (amends ADR-0042) | session | Coalesce preedit with latest-wins semantics bounded by short deadlines, and let commit text bypass the deadline. Invariant: every time-based wake is an explicit deadline merged into the poll timeout. | 2026-07-11 | [Input Session](../architecture/input-session.md) |
| [0044](0044-bounded-panel-rendering.md) | Bounded Panel rendering before SHM presentation | Accepted (supersedes ADR-0013's grow-only sizing) | panel | Quantise and shrink the render extent, and reserve a free shared-memory buffer before doing any render work. Invariant: render cost tracks recent content, and a frame is skipped rather than overwritten when buffers are busy. | 2026-07-12 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0045](0045-rust-settings-workspace-integration.md) | Rust Settings Workspace Integration | Accepted (supersedes ADR-0035 D5 and ADR-0039's deferral) | control | Make the settings application a workspace crate that mutates state only through the control protocol, editing just its own styling file directly. Invariant: one control surface for state mutation. | 2026-07-13 | [Control Plane and Clients](../architecture/control-and-clients.md) |
| [0046](0046-engine-protocol-only-runtime.md) | Engine-Protocol-Only Runtime | Accepted (supersedes ADR-0038's ABI decision; retires the C ABI) | engine | Make the typed engine protocol the only engine boundary: manifest plus worker executable, no shared library, no C ABI, no compatibility shim. Invariant: nothing but the protocol crosses the engine boundary. | 2026-08-03 | [Engine Runtime](../architecture/engine-runtime.md) |
| [0047](0047-headless-platform-state-decoupling.md) | Headless Platform State Decoupling and CI Test Resilience | Accepted | session | Encapsulate transport proxies so platform state constructs and functions without a live display. Invariant: lifecycle and policy tests run in CI unconditionally, with no display and no skipped tests. | 2026-09-11 | [Input Session](../architecture/input-session.md) |
| [0048](0048-optics-lens-alignment-and-bounded-key-budget.md) | Modern Optics Lens Component Alignment and Bounded Keystroke Latency Budget | Accepted (refines ADR-0041's engine-IPC bound) | performance | Bound hot-path keystroke processing to a tight timeout with poison recovery and pass-through. Invariant: a stalled engine cannot freeze the Wayland loop for longer than the keystroke budget. | 2026-09-11 | [Daemon Lifecycle](../architecture/daemon-lifecycle.md) |
| [0049](0049-toml-only-configuration.md) | TOML-only configuration | Accepted | process | Accept exactly one configuration dialect. Invariant: a malformed configuration is a parse error, never a silently partial tree. | 2026-09-11 | [Daemon Lifecycle](../architecture/daemon-lifecycle.md) |
| [0050](0050-panel-typeface-family-class.md) | Panel typeface family is a family *class* | Accepted (supersedes the arbitrary-family reading of ADR-0016 D3) | panel | Model the panel typeface setting as the class the text stack can actually apply. Invariant: every value the UI offers and the config accepts changes rendering. | 2026-09-11 | [Panel Rendering](../architecture/panel-rendering.md) |
| [0051](0051-single-documentation-tree.md) | Single documentation tree at the repository root | Accepted | process | Keep one documentation tree at `docs/`, with per-crate trees retired. Invariant: a document has exactly one home in the 4D tensor. | 2026-09-11 | [Workspace Topology](../architecture/workspace-topology.md) |
| [0052](0052-ordered-keyboard-ownership.md) | Ordered keyboard events and press ownership | Accepted (amends ADR-0003; supersedes ADR-0018 decision 1) | session | Queue keys, modifiers, and boundaries together; one virtual-keyboard ledger pairs presses across focus changes. Invariant: text-state batches cannot erase a focus edge or lose an owned release. | 2026-09-15 | [Input Session](../architecture/input-session.md) |

## Retired records

Retired decisions keep their row above (so citations resolve) and their
authority here. Nothing is deleted ([INV-TEMP-03](../governance/documentation/core/invariants.md) /
[INV-TEMP-04](../governance/documentation/core/invariants.md)). The fully
superseded records in this set are ADR-0004, 0007, 0010, 0013, 0019, 0024, 0027,
0028, 0036, and 0037.

| Set | Location | Covers |
| :--- | :--- | :--- |
| Framework core ADRs (18 records) | [`archive/framework-core/adr/`](archive/framework-core/adr/index.md) | The retired C-ABI framework: plugin engine loading, the composition contract, engine properties, mode reflection, host-managed selection |
| Framework core dev notes | [`archive/framework-core/dev/`](archive/framework-core/dev/index.md) | Retired header layers and ABI stability policy, plus keyboard-status provenance |
| CLI control ADRs (5 records) | [`archive/cli-control/`](archive/cli-control/index.md) | The standalone CLI repository: independence, binary naming, the resource+verb schema |

The archive has its own charter in [`archive/index.md`](archive/index.md).

## Compaction status

The compaction triggers are evaluated from this registry:

| Trigger | Threshold | Status |
| :--- | :--- | :--- |
| Subsystem saturation | 5 or more incremental or amending records in one subsystem | **Met** for panel rendering and for the engine boundary |
| Repository milestone | More than 50 active records | Not met (41 active records; 10 are fully superseded) |
| High invalidation ratio | More than 40% superseded or deprecated | Not met |

The living snapshots exist ([Architecture Blueprints](../architecture/index.md)),
which is compaction step 1. Steps 2–4 — tombstoning a record's status, relocating
it into `archive/`, and repointing its registry row — are deliberately left to a
maintainer-led pass per subsystem, because they rewrite the front matter of
`Accepted` records. Until that pass happens, the records above remain the active
authority, and each blueprint's lineage table cites them.

## Historical provenance

Workspace settings decisions start at ADR-0045; earlier settings-panel decisions
remain in the archived sibling checkout.

## See also

- [Architecture Blueprints](../architecture/index.md) — what the system is today
- [ADR entity rules](../governance/documentation/profiles/architecture/adr.md) — admission filter, lifecycle, template
- [Rationale for the consolidation above](0051-single-documentation-tree.md)
