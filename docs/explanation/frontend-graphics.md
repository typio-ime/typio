# Frontend Graphics

The host renders one floating UI: the **Panel**. Candidate composition,
engine/mode indicators, and voice status share the same input-popup
`wl_surface`; the active producer is selected by the Panel Coordinator before
rendering starts.

Rendering uses [flux](../../flux)'s **software (CPU) canvas** plus a pure-Rust
text rasteriser; there is no Vulkan device or WSI in the Panel path:

```text
composition / indicator / voice state
  -> Panel Coordinator ownership and anchor policy
  -> FluxPanel sizing and layout
  -> flux_canvas CPU draw commands (background + selection highlight)
  -> text_raster composites glyphs into the RGBA8 framebuffer
  -> host-managed wl_shm buffer
  -> wl_surface.attach + damage_buffer + commit
```

See [ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md) for the decision
to render the Panel on the CPU (flux software canvas + a pure-Rust text
rasteriser) and present over `wl_shm` only.

## Rendering Boundaries

### Policy

Frontend policy lives in `crates/typio-host/src/panel_coordinator.rs` and the
event-loop caller. It decides:

- which UI owner may show;
- whether positioned UI has a trusted anchor;
- whether an anchor probe is needed;
- when a dirty candidate snapshot should be flushed.

This layer owns Wayland focus and input-method protocol facts. It does not draw.

### Render

`crates/typio-host-platform/src/panel.rs` owns the flux CPU canvas and the text
rasteriser:

- the flux **CPU canvas** (`flux_canvas_create_cpu` + `flux_canvas_cpu_begin`/
  `end` + `flux_canvas_fill_rrect`) — fills the panel background and selection
  highlight into a premultiplied RGBA8 framebuffer on the host;
- **flux-text** CPU text shaping/rasterisation (`TextRaster`, see
  [`text_raster.rs`](../../crates/typio-host-platform/src/text_raster.rs), backed by
  `flux-text-sys` — FreeType/HarfBuzz/Fontconfig) — shapes and rasterises
  glyphs, compositing them directly into that same RGBA8 framebuffer;
- a 64×32-pixel quantized surface cropped to the exact content extent via
  `wp_viewport` when available; capacity shrinks with hysteresis after a wide
  page is no longer needed.

`FluxPanel::draw_candidates()` and `FluxPanel::draw_status_banner()` record the
canvas commands and the text draws. There is **no Vulkan device, no
offscreen `flux_surface`, no GPU→CPU readback, no glyph texture upload** in this
path.

### Present

`crates/typio-host-platform/src/panel_shm.rs` owns the triple-buffered SHM pool.
A buffer is selected before CPU drawing. The filled framebuffer is byte-swapped
(RGBA8 → Wayland ARGB8888) into that buffer's memory, then its `wl_buffer` is
attached to the popup surface via raw `wl_surface.attach` /
`damage_buffer` / `commit`. If the compositor has not released any buffer, the
draw is skipped; the event loop remains free to process input and later
render the newest coalesced state.

No `wl_surface.frame` pacing gate sits in this path. If every SHM buffer is
busy, the current frame is dropped and the latest dirty candidate snapshot is
retried after a later input or buffer-release event.

## Flux Dependency Boundary

The host's graphics dependencies are flux's **CPU canvas** and flux-text's
host-resident R8 coverage path. It does **not** depend on flux's Vulkan device,
surface, swapchain, or GPU readback APIs:

| Concept | Current use |
|---|---|
| `flux_canvas_create_cpu` / `flux_canvas_cpu_begin` / `end` | CPU canvas lifecycle — fills and rounded-rects into a host framebuffer. |
| `flux_canvas_cpu_pixels` | Direct pointer into the RGBA8 framebuffer (no bus, no fence). |
| `flux_canvas_fill_rrect` | Background and selection-highlight fills. |
| `flux_text_draw` | Shapes text and composites host-resident R8 glyph coverage into the CPU canvas. |

Text is drawn by **flux-text** (via `TextRaster`); glyphs composite into the
flux framebuffer directly. (The `rustybuzz`/`fontdb`/`ab_glyph` stack backs
only the tray badge `icon_badge`, behind the `systray` feature — not the panel.)

Porting to another canvas backend would require replacing `FluxPanel`'s
canvas fill calls and the `TextRaster` compositing step. Ownership policy,
anchor handling, key routing, and engine state do not depend on flux.

## Input Correctness

Rendering is downstream of input. A skipped or delayed frame can make the
visible highlight lag briefly, but it does not change which candidate is
selected or committed. Key events remain ordered on the Wayland event loop, and
the next successful Panel frame consumes the latest coalesced composition
snapshot.

## See Also

- [Panel Architecture](panel-architecture.md) — ownership and anchor policy.
- [Candidate Panel Behavior](candidate-panel-behavior.md) — user-visible Panel
  lifecycle.
- [ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md) — CPU canvas render
  and host-managed SHM buffers.
- [ADR-0014](../adr/0014-canonical-panel-vocabulary.md) — canonical Panel
  terminology.
