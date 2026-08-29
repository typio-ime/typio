# ADR-0040: CPU Canvas Render with Host-Managed SHM Buffers

- **Status**: Accepted (supersedes ADR-0010 and ADR-0013; supersedes the
  former dmabuf zero-copy present path and the Vulkan offscreen render path)
- **Date**: 2026-01-12
- **Deciders**: typio host maintainers

## Context

The candidate panel and status banners historically rendered through flux's
Vulkan backend: an offscreen `flux_surface` recorded a frame, the result was
read back over the GPU→CPU bus (`flux_surface_read_pixels`) and copied into a
host-owned `wl_shm` `wl_buffer`. An optional zero-copy dma-buf export path
(`zwp_linux_dmabuf_v1`, `TYPIO_PANEL_DMABUF=1`) attached the GPU image
directly. This whole pipeline carried a Vulkan device, the readback stall, and
a dmabuf path that several compositors silently dropped.

flux has since gained a **software (CPU) canvas backend**
(`flux_canvas_create_cpu`; see `optics` ADR-0019) that rasterises fills,
gradients and rounded-rects on the host into a premultiplied RGBA8
framebuffer — no Vulkan device, surface, swapchain, or dma-buf. The CPU
backend does **not** draw glyphs (they need GPU-resident textures), so text
needs its own rasteriser regardless.

## Rationale

The choice of **CPU rendering + `wl_shm`** is a deliberate technology
selection, not a fallback. The reasoning runs from the present path
backward to the renderer:

- **The present path is fixed first, and it is `wl_shm`.** `wl_shm` is
  the only present mechanism the Wayland core protocol mandates and every
  compositor implements. `zwp_linux_dmabuf_v1` is an optional extension
  whose behavior on input-popup surfaces varies by compositor; several
  compositors were observed to *silently drop* input-popup dma-buf
  buffers with no protocol feedback — an undetectable failure mode for a
  UI the user is actively typing through. Vulkan WSI present
  (`vkQueuePresentKHR`) is a blocking queue operation that stalls the
  calling thread when a compositor stops recycling swapchain images.
- **The workload does not benefit from GPU parallelism.** A frame is one
  background rounded-rect, one highlight rounded-rect, and a row of
  pre-shaped glyphs drawn into a framebuffer on the order of a megabyte,
  redrawn on keystrokes rather than at animation rates. There are no
  per-pixel effects (blur, composited layers) for a shader to accelerate,
  so GPU rasterisation has nothing to win back.
- **The renderer belongs on the same side as the destination.** Since the
  destination is host memory (`wl_shm`), any GPU renderer must pay a
  GPU→CPU readback or a dma-buf export on every frame — a fixed overhead
  that exceeds the entire CPU rasterisation cost at this size. A GPU
  pipeline also adds a Vulkan device lifecycle, device-lost recovery
  after suspend, driver-dependent behavior, worse latency variance, and
  waking the GPU on every keystroke.
- **Field evidence matches the structural analysis.** The earlier Vulkan
  offscreen + readback path and the dma-buf zero-copy path shipped and
  repeatedly broke across compositors, drivers, and resume-from-suspend,
  including a watchdog kill from swapchain exhaustion. Those failures are
  instances of the structural problems above, not the sole reason for the
  decision.

This is a component-level selection, not a verdict on GPU rendering in
general. The settings GUI renders through the GPU stack (Iris/Lens/Flux;
see `crates/typio-settings`). GPU rendering becomes the right choice for
the Panel if the workload changes — continuous animation, per-pixel
effects such as blur, or surfaces large enough that software
rasterisation misses the frame budget — and any such move must keep
`wl_shm` as the fallback present path. See
[Frontend Graphics](../explanation/frontend-graphics.md) for the current
pipeline.

The cost accepted in return is single-threaded software rasterisation
with no GPU parallelism — well within frame budget for a single
candidate row (see Consequences).

## Decision

Render the panel entirely on the CPU and present over `wl_shm` only:

1. **Background + selection highlight** — `flux_canvas_create_cpu` +
   `flux_canvas_cpu_begin/end` + `flux_canvas_fill_rrect`. The framebuffer is
   read back from `flux_canvas_cpu_pixels` (no bus, no fence).
2. **Text** — flux-text's CPU shaping + rasterisation, accessed via
   `flux-text-sys` (`crates/typio-host-platform/src/text_raster.rs`). Text draws
   directly into the RGBA8 canvas pass via the host-coverage glyph path
   (ADR-0019) — no GPU image required. The panel shares flux-text's
   FreeType/HarfBuzz/Fontconfig backend with the rest of the host.
3. **Present** — the framebuffer is byte-swapped (RGBA8 → Wayland ARGB8888)
   into a host-managed `wl_shm` `wl_buffer` and attached to the popup
   `wl_surface` via raw `wl_surface.attach`/`damage_buffer`/`commit`.

Removed: the Vulkan `flux_device`/`flux_surface` lifecycle, the GPU→CPU
readback, the "liquid glass" effect, `panel_dmabuf.rs`, the
`zwp_linux_dmabuf_v1` protocol wiring, and the `TYPIO_PANEL_DMABUF` opt-in.
`wl_shm` is now the only present path. (`flux-text`/`flux-text-sys` are
**retained** for CPU text shaping/rasterisation — see Decision 2.)

Grow-only framebuffer sizing with `wp_viewport` cropping (from ADR-0013) is
retained. The panel extent is hard-capped (16384 logical px wide / 4096 px
tall): candidate rows wider than the cap render a fitting prefix plus a `⋯`
overflow marker, and over-long banner labels truncate with `…`. The cap is a
trust boundary — engine replies drive the panel width, and without a bound a
buggy or hostile engine could imply multi-gigabyte allocation attempts per
frame. Buffer busy/release tracking rides in each `wl_buffer`'s own wayland
user-data (`Dispatch<wl_buffer, BufferReleaseState>`), so a release event
always clears exactly the buffer it belongs to.

## Alternatives considered

- **Keep Vulkan offscreen + readback**: rejected — the destination is host
  memory either way, so this pays a per-frame readback stall larger than
  the whole CPU rasterisation cost at this size, plus the Vulkan device
  lifecycle, for no present-path gain.
- **Drop flux from the panel and reimplement rounded-rect fills in Rust**:
  rejected as needless rework of geometry flux already rasterises well.
- **dma-buf zero-copy**: rejected — `zwp_linux_dmabuf_v1` is an optional
  extension, and several compositors silently drop input-popup dmabuf
  buffers with no protocol feedback, an undetectable failure mode. The
  SHM path is universally supported.

## Consequences

- Positive: no GPU dependency for panel rendering; no readback stall; one
  universally-supported present path; no dmabuf protocol wiring.
- Trade-off: panel text quality depends on flux-text's CPU rasteriser
  (FreeType/HarfBuzz), which is shared with the rest of the host,
  including the tray badge (`icon_badge`, behind the `systray` feature).
  No pure-Rust shaping stack (`rustybuzz`/`fontdb`/`ab_glyph`) remains in
  the workspace.
- Negative (accepted): software rasterisation is single-threaded and does not
  benefit from GPU parallelism; for a single candidate row this is well within
  frame budget.
