# Subsystem Architecture: Panel Rendering

- Status: Living Blueprint
- Last Updated: 2026-09-15
- Scope: core/panel — Panel content model, UI-owner arbitration, render scheduling, CPU canvas rendering, and `wl_shm` presentation for the host's single floating IME surface
- Maintainers: Typio maintainers

---

## 1. System Overview & Boundaries

The host owns **one** floating IME surface: the **Panel**. Candidate lists, engine/profile indicators, and voice status are different *owners* of that surface, not different windows ([ADR-0005](../adr/0005-unified-panel-backend.md), [ADR-0014](../adr/0014-canonical-panel-vocabulary.md)). `zwp_input_method_v2` permits only one `zwp_input_popup_surface_v2` per input-method object, so multi-zone composition inside one surface is the only available design.

The subsystem is split along the seam ADR-0005 made load-bearing — platform-free versus platform-bound:

| Tier | Responsibility | Platform dependency |
| :--- | :--- | :--- |
| Frontend policy | Who may show the Panel, anchor readiness, anchor probe, when a dirty snapshot is flushed | Wayland focus and input-method facts |
| Pure decision helpers | Schedule state, presentation de-duplication, preedit/panel sync plan, anchor generation bookkeeping | None; unit-testable without a display server |
| Render + present | Content to geometry to pixels, `wl_shm` buffer reservation and attach | Wayland surface, `wl_shm`, flux CPU canvas, flux-text |

The boundary stops in four places:

- **Ownership policy is not rendering.** `panel_coordinator` decides *whether* the Panel may be shown; the renderer only turns an approved request into pixels. No renderer entry point consults owner state.
- **Rendering is downstream of input.** A dropped frame changes only what is visible; it never changes the composition, the selection, or committed text.
- **Inline preedit is not a Panel zone.** The daemon sends preedit through `zwp_input_method_v2.set_preedit_string` and the focused client renders it at the insertion point, so preedit and candidates travel independent paths.
- **The engine is not trusted for extent.** Candidate strings arrive from engine processes; the Panel treats them as untrusted input to a hard extent cap.

The persistent-object model of ADR-0014 (exactly four process-lifetime objects) still governs, but its realisation changed with the move to CPU rendering: `RenderDevice` no longer exists because the CPU canvas has no device, and `PanelSurface` is no longer a separate object. The process-persistent objects today are:

1. the popup `wl_surface`, created and owned by the platform frontend (`crates/typio-host-platform/src/input_method.rs`);
2. `FluxPanel` (`crates/typio-host-platform/src/panel.rs`) — canvas, quantized extent, layout cache, and the panel font config;
3. the single `TextRaster` inside `FluxPanel` (`crates/typio-host-platform/src/text_raster.rs`) — one flux-text context;
4. `ShmBufferPool` (`crates/typio-host-platform/src/panel_shm.rs`) — the fixed triple-buffered `wl_shm` pool.

Everything else is a value: `flux_canvas`, the per-candidate `TextMetrics` layout, the quantized extent, and the SHM framebuffer contents. The multi-zone vocabulary survives as two draw entry points: `FluxPanel::draw_candidates` (Candidate Zone) and `FluxPanel::draw_status_banner` (Status Zone). Status content is a single label string; there is no generic content model type in the tree today.

## 2. Invariants & Non-Negotiable Rules

- **`[INV-PANEL-01]` Single visible owner.** At most one `UiOwner` (`Candidate`, `Indicator`, `Voice`) is the visible Panel owner at any time, and the coordinator is the only component that may change it (`PanelCoordinator::claim`, `hide`, `decide_positioned_flush`, `hide_all` in `crates/typio-host-types/src/panel_coordinator.rs`).
- **`[INV-PANEL-02]` Hide is owner-scoped.** A hide request only clears the owner that issued it; a stale timer or release event from a previous owner must not hide the current owner (`PanelCoordinator::hide`, and `app/panel_driver.rs::hide_candidate_panel`, which refuses to detach a buffer while an overlay owner is visible).
- **`[INV-PANEL-03]` A later owner replaces the current owner.** Ownership is temporal, not prioritised; a new position request supersedes the visible owner and resets the pending timer.
- **`[INV-PANEL-04]` Candidates cancel pending positioned UI.** A candidate claim drops any queued Indicator/Voice request, so typing never waits behind out-of-band status UI.
- **`[INV-PANEL-05]` Every activation has a fresh anchor generation.** `reset_anchor` bumps the generation and invalidates presentation; the generation is ready only after a `text_input_rectangle` for it or a successful candidate flush, and no positioned status UI may be mapped before readiness or the bounded fallback deadline.
- **`[INV-PANEL-06]` The renderer holds no ownership or anchor policy.** `panel.rs` and `panel_shm.rs` must not read `UiOwner`, pending-request state, or caret-rect facts; those decisions arrive as an already-approved draw call.
- **`[INV-PANEL-07]` Only the event loop flushes the Panel.** Key routing and composition callbacks may only mark the schedule state `Dirty` (`PanelScheduleState`); presentation happens in `app/panel_driver.rs::flush_candidate_panel` during the reactor step.
- **`[INV-PANEL-08]` A free SHM buffer is reserved before any render work.** `draw_candidates` acquires the buffer before canvas creation, clearing, text rasterisation, or supersample resolve; on `ShmAcquireError::Busy` it returns `false` without touching the canvas and the schedule state stays `Dirty`.
- **`[INV-PANEL-09]` Buffer-release tracking is per buffer.** The busy flag lives in the `wl_buffer`'s own wayland user-data (`BufferReleaseState`), and the `Dispatch<wl_buffer, BufferReleaseState>` handler clears exactly the buffer the event arrived on — no proxy-pointer registry, no cross-lookup.
- **`[INV-PANEL-10]` Extent is quantized, hysteretic, and capped.** With `wp_viewporter` the framebuffer is quantized to 64 px (width) and 32 px (height), grows to the required quantized size, and shrinks only when current content needs no more than half of an axis (`retained_extent`); without a viewport the extent is exact.
- **`[INV-PANEL-11]` The Panel extent is hard-capped at 16384 × 4096 logical pixels.** Candidate pages beyond the cap render a fitting prefix plus a `⋯` overflow marker and at least one candidate stays visible; over-long banner labels truncate with `…`. Both branches are trust boundaries against buggy or hostile engines.
- **`[INV-PANEL-12]` A skipped frame never changes logical state.** Back-pressure keeps the schedule state `Dirty` and retries only the newest coalesced snapshot; committed text and selection are unaffected by any number of dropped frames.
- **`[INV-PANEL-13]` No GPU object exists in the Panel path.** No Vulkan device, `flux_surface`, swapchain, dma-buf, GPU readback, or GPU glyph texture may be introduced; fills come from `flux_canvas_create_cpu` and text from the flux-text host-coverage path, and the only present mechanism is `wl_shm` with `attach`/`damage_buffer`/`commit`.

## 3. Component Architecture & Data Flow

### 3.1 Producers and the coordinator

| Owner | Producer | Entry point | Content |
| :--- | :--- | :--- | :--- |
| `UiOwner::Candidate` | keyboard composition | `app/panel_driver.rs::flush_candidate_panel` | candidate row, selection highlight, index labels |
| `UiOwner::Indicator` | engine/profile change | `app/indicator.rs::render_indicator_banner` | single status label |
| `UiOwner::Voice` | voice session | `app/indicator.rs::render_voice_status_banner` | recording / processing / unavailable / error label |

`PanelCoordinator` (`crates/typio-host-types/src/panel_coordinator.rs`) holds the arbitration state: `ui_owner`, `position_anchor_generation`, `position_anchor_ready_generation`, `position_anchor_probe_generation`, `position_anchor_has_caret`, and the pending-request fields (`positioned_ui_pending*`).

| Coordinator member | Rule it implements |
| :--- | :--- |
| `claim(owner)` | A later owner supersedes the visible owner; claiming `Candidate` cancels a pending status request |
| `decide_positioned_flush(owner, label)` | `Candidate` shows immediately with fallback placement and marks the anchor ready; `Indicator`/`Voice` queue until the anchor is ready |
| `flush_pending_with_timeout(now)` | On anchor deadline expiry, marks the anchor ready and shows rather than stranding the popup; applies to every owner |
| `anchor_deadline_remaining_ms(now)` | Feeds the pending anchor deadline into the poll timeout (`app/event_loop.rs`) |
| `hide(owner)` / `hide_all()` | Owner-scoped hide, and teardown of all Panel ownership |
| `should_probe_anchor()` / `record_probe_sent()` | One anchor probe per generation, at most |

Anchor readiness facts arrive from two sources: the `zwp_input_popup_surface_v2` `text_input_rectangle` handler (which calls `note_caret_rect` and `mark_anchor_ready`), and a successful candidate flush. The anchor probe itself is a no-op text transaction — `probe_anchor` calls `text_transaction_and_flush(None, Some(("", 0)))`, which is `set_preedit_string("")` followed by `commit(serial)` — and exists to make clients that refresh caret rectangles on input-method traffic send a fresh rectangle.

`PanelCoordinatorConfig::from_values` clamps the probe timeout to 10–1000 ms, treating anything below the minimum as the 20 ms coordinator default; the shipped config defaults (`display.anchor_probe`, `display.anchor_probe_timeout_ms`) come from `crates/typio-settings/src/platform_config.rs`.

### 3.2 Scheduling

`PanelScheduleState` (`crates/typio-host-types/src/panel_scheduler.rs`, re-exported flat from the crate root) has two states: `Idle` and `Dirty`. Candidate composition callbacks and host-managed navigation only mark it `Dirty`; `flush_candidate_panel` is the sole consumer.

```text
composition / navigation  ->  mark_panel_dirty()          (Dirty)
reactor step              ->  flush_candidate_panel()
                                 not focused / no context  -> complete() (Idle), discard
                                 zero candidates           -> hide, complete()
                                 already presented         -> complete(), skip
                                 pool busy / draw declined -> stay Dirty
                                 frame attached            -> complete(), Idle
```

`PresentationRecord` (`crates/typio-host-types/src/panel_present_gate.rs`) de-duplicates presentation by `(generation, composition_seq)`: `invalidate()` is called on scale, theme, ownership, focus and anchor changes that require a repaint even though the composition sequence did not change.

### 3.3 Render and present

| Component | File | Responsibility |
| :--- | :--- | :--- |
| `FluxPanel` | `crates/typio-host-platform/src/panel.rs` | layout cache, extent policy, CPU canvas lifecycle, `draw_candidates`, `draw_status_banner`, `present_shm`, `hide`, `reset_shm_pool`, `set_scale`, `set_font_config` |
| `TextRaster` | `crates/typio-host-platform/src/text_raster.rs` | one `flux_text*` context; `measure` and `draw` into the canvas via the host-coverage glyph path; family-class selection |
| `ShmBufferPool` / `ShmBuffer` | `crates/typio-host-platform/src/panel_shm.rs` | fixed-capacity (`DEFAULT_CAP = 3`) pool, non-blocking `acquire`, per-buffer `BufferReleaseState`, `reset` |
| Buffers and canvas | — | RGB(A) data only; no draw commands survive a frame |

Layout is measured per candidate into `Vec<(TextMetrics, TextMetrics)>` (index label plus candidate text), keyed by a `LayoutCacheKey` over `(scale bits, candidate count, content hash)`. Font configuration changes and scale changes invalidate that cache. Text metrics are logical pixels; the CPU canvas carries the content-scale transform so HiDPI rasterises from the same coordinates.

Extent policy, applied by `apply_surface_size` and shared by both draw paths:

```text
required  = ceil(content extent x scale)          physical pixels
quantized = ceil(required / quantum) * quantum    quantum = 64 (w), 32 (h)
retained  = quantized   if current == 0
                        or required > current
                        or required * 2 <= current
            current     otherwise                 (hysteresis: shrink at <= half)
viewport.set_source(0, 0, phys_w, phys_h)         when wp_viewporter is bound
viewport.set_destination(content_w, content_h)
```

Full-frame repaint is the model: each frame clears, fills the background rounded-rect, fills the selection rounded-rect, and draws every visible glyph run. A reserve-then-draw present sequence:

```text
draw_candidates(candidates, selected, composition_seq)
  1. ensure_candidate_size()      layout + quantized extent + viewport update
  2. acquire_shm_buffer()         Busy -> return false, stay Dirty (no render work)
  3. ensure_canvas()              lazy flux_canvas_create_cpu at the retained extent
  4. flux_canvas_begin
       fill_rrect (background), fill_rrect (selection), TextRaster::draw (per run)
     flux_canvas_end
  5. present_shm(buffer_index)
       flux_canvas_cpu_pixels -> byte-swap RGBA8 -> ARGB8888 straight into the buffer
       mark_busy(), set_buffer_scale, attach + damage_buffer + commit
```

Path tracing under `typio.panel.perf` reports per-phase microsecond timings (`layout_us`, `acquire_us`, `draw_us`, `present_us`), and `typio.panel.shm` reports pool exhaustion and allocation failures.

Panel backgrounds and selection highlights use Flux geometry and solid brushes.
Canvas and text contexts release their references through the current ownership
APIs, including resize and early-return paths. The workspace and CI native pin
select Optics v0.0.44; see [Workspace Topology](workspace-topology.md).

### 3.4 Back-pressure and convergence

The compositor's only back-pressure signal on this path is `wl_buffer.release`. `ShmBufferPool::acquire` returns `ShmAcquireError::Busy` when every buffer is held; the Panel then renders nothing, keeps `Dirty`, and a later `wl_buffer` release or input event retries the **newest** composition snapshot. Because `mark_busy` is written into the buffer's own user-data at `create_buffer` time, a release can never clear the wrong buffer's flag.

`FluxPanel::reset_shm_pool` clears every cached buffer after a hard lifecycle boundary (grab destroy / resume), covering compositors that lose or indefinitely delay `wl_buffer.release` for buffers in flight across suspend. Without that reset a permanently busy pool would drop every later candidate frame.

### 3.5 Retired mechanisms — do not reintroduce

| Retired | Why it must not return |
| :--- | :--- |
| Vulkan offscreen render plus GPU→CPU readback | The destination of every frame is host memory (`wl_shm`), so readback costs more than the entire CPU rasterisation at this size, and adds a device lifecycle and suspend recovery path ([ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md)) |
| `zwp_linux_dmabuf_v1` zero-copy present | Optional extension; several compositors silently drop input-popup dma-buf buffers with no protocol feedback — an undetectable failure mode for a surface the user types through ([ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md)) |
| `wl_surface.frame` as a hard present gate | A compositor may withhold callbacks for an input-popup surface; treating them as a liveness contract froze candidate updates until a recovery timer ([ADR-0036](../adr/0036-soft-present-gate-for-candidate-panel.md), [ADR-0044](../adr/0044-bounded-panel-rendering.md)) |
| Indefinitely grow-only buffers | With a CPU renderer, clear/resolve/copy cost scales with allocated pixels, so one wide page would set the per-frame cost for the whole session. Quantized retention with hysteresis shrink replaces it ([ADR-0044](../adr/0044-bounded-panel-rendering.md)) |
| GPU glyph atlases and per-text-run coverage textures | They belonged to the Vulkan renderer; the CPU canvas has no glyph texture at all ([ADR-0011](../adr/0011-colour-independent-coverage-glyphs.md), [ADR-0012](../adr/0012-glyph-atlas-shared-texture.md), [ADR-0019](../adr/0019-atlas-hash-compaction.md), [ADR-0020](../adr/0020-atlas-reclamation-and-glyph-layer-modularization.md)) |
| Durable retry state on the panel surface | A persistent retry flag could latch and permanently suppress renders; retry scheduling is owned by the update path, not by the surface ([ADR-0022](../adr/0022-panel-retry-result-owned-by-update.md)) |
| Rendering inside the key path | Flushing presentation from key routing pulls GPU/compositor stalls into input latency ([ADR-0023](../adr/0023-panel-scheduler-state-machine.md)) |

## 4. Historical Lineage & Founding ADRs

- Compaction status: **no record has been compacted yet.** Every decision below is still an active record in [the ADR index](../adr/index.md); nothing has been tombstoned, relocated to `docs/adr/archive/`, or superseded by this blueprint, and no ADR carries a `Compacted into` status. The compaction trigger for this subsystem is met (more than five amending records), but the tombstone-and-relocate pass has not run.

| Record | What it established |
| :--- | :--- |
| [ADR-0005: Unified Panel Backend for Candidate and Status UI](../adr/0005-unified-panel-backend.md) | One surface composites multiple zones; the content model must stay free of Wayland/GPU types; layout and paint stay decoupled |
| [ADR-0006: Resilient Candidate-Popup GPU Present](../adr/0006-resilient-candidate-popup-present.md) | Bounded present and a consecutive-timeout recovery streak, so a stalled compositor cannot freeze the loop; input correctness survives a frozen highlight |
| [ADR-0010: Non-blocking present mode for the candidate popup](../adr/0010-non-blocking-candidate-popup-present.md) | The steady-state present-side block was a separate cost from the acquire stall (superseded by ADR-0040) |
| [ADR-0011: Colour-independent coverage glyph textures](../adr/0011-colour-independent-coverage-glyphs.md) | Glyph textures stop baking colour; colour becomes a draw-time tint |
| [ADR-0012: Shared glyph atlas](../adr/0012-glyph-atlas-shared-texture.md) | Rasterise once, reference sub-rects; per-text-run textures are the anti-pattern to avoid |
| [ADR-0013: Grow-only popup swapchain](../adr/0013-grow-only-popup-swapchain.md) | Size quantisation and grow-only retention to stop per-page recreation (Vulkan part superseded by ADR-0040) |
| [ADR-0014: Canonical Panel Vocabulary and Module Ontology](../adr/0014-canonical-panel-vocabulary.md) | The object model and the four persistent objects; `popup` is reserved for the protocol role |
| [ADR-0015: Candidate popup lag — final fixes](../adr/0015-candidate-popup-lag-final-fixes.md) | Bounded acquire budget, deferral of work while retrying, microsecond instrumentation in the present path |
| [ADR-0017: Positioned UI arbitration for panel owners](../adr/0017-positioned-ui-arbitration.md) | One visible owner, later-owner replacement, owner-scoped hide, anchor generations, and the anchor probe |
| [ADR-0019: Atlas hash-table compaction](../adr/0019-atlas-hash-compaction.md) | Hash-table load bounding for sustained CJK input (superseded by ADR-0020); a cautionary record of an inverted root cause |
| [ADR-0020: Atlas texture reclamation and glyph-layer modularization](../adr/0020-atlas-reclamation-and-glyph-layer-modularization.md) | Reclamation must return the underlying resource; per-module cache lifetime contracts |
| [ADR-0022: Panel retry result owned by update](../adr/0022-panel-retry-result-owned-by-update.md) | Retry is the result of one update, not durable surface state |
| [ADR-0023: Panel Scheduler State Machine](../adr/0023-panel-scheduler-state-machine.md) | Explicit schedule state instead of a boolean; only the event loop may flush the Panel |
| [ADR-0036: Soft Present Gate for the Candidate Panel](../adr/0036-soft-present-gate-for-candidate-panel.md) | Frame callbacks are pacing hygiene, never a hard lock (superseded by ADR-0040) |
| [ADR-0040: CPU Canvas Render with Host-Managed SHM Buffers](../adr/0040-cpu-canvas-render-shm-buffers.md) | CPU canvas plus flux-text, `wl_shm` as the only present path, per-buffer release tracking, and the hard extent cap |
| [ADR-0044: Bounded Panel Rendering Before SHM Presentation](../adr/0044-bounded-panel-rendering.md) | Reserve-before-render, quantized extent with hysteresis shrink, and latest-snapshot convergence on back-pressure |
| [ADR-0050: Panel typeface family is a family class](../adr/0050-panel-typeface-family-class.md) | `display.font_family` is a supported family class (`default`/`sans`/`serif`/`mono`), not an arbitrary family name; every accepted value changes rendering |

## See Also

- [Panel Architecture](../explanation/panel-architecture.md) — why ownership arbitration sits before rendering
- [Candidate Panel Behavior](../explanation/candidate-panel-behavior.md) — user-visible Panel lifecycle
- [Frontend Graphics](../explanation/frontend-graphics.md) — render pipeline and Flux dependency boundary
- [Panel Appearance](../dev/panel-appearance.md) — appearance, font, theme, and cache notes
- [Glossary](../reference/glossary.md) — Panel, owner, and anchor terms
