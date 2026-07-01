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

## Decision

Render the panel entirely on the CPU and present over `wl_shm` only:

1. **Background + selection highlight** — `flux_canvas_create_cpu` +
   `flux_canvas_cpu_begin/end` + `flux_canvas_fill_rrect`. The framebuffer is
   read back from `flux_canvas_cpu_pixels` (no bus, no fence).
2. **Text** — a pure-Rust CPU rasteriser (`crates/typio-host/src/text_raster.rs`,
   built on `rustybuzz` + `fontdb` + `ab_glyph`, the same stack already used by
   `icon_badge`) composites glyphs directly into the RGBA8 framebuffer. The
   `flux-text` glyph atlas / `flux-text-sys` dependency is removed from the
   panel entirely.
3. **Present** — the framebuffer is byte-swapped (RGBA8 → Wayland ARGB8888)
   into a host-managed `wl_shm` `wl_buffer` and attached to the popup
   `wl_surface` via raw `wl_surface.attach`/`damage_buffer`/`commit`.

Removed: the Vulkan `flux_device`/`flux_surface` lifecycle, the GPU→CPU
readback, `flux-text`/`flux-text-sys`, the "liquid glass" effect,
`panel_dmabuf.rs`, the `zwp_linux_dmabuf_v1` protocol wiring, and the
`TYPIO_PANEL_DMABUF` opt-in. `wl_shm` is now the only present path.

Grow-only framebuffer sizing with `wp_viewport` cropping (from ADR-0013) is
retained.

## Alternatives considered

- **Keep Vulkan offscreen + readback**: rejected — flux's CPU canvas removes
  the device and readback stall entirely while still giving us flux-quality
  fills/rounded-rects.
- **Drop flux from the panel and reimplement rounded-rect fills in Rust**:
  rejected as needless rework of geometry flux already rasterises well.
- **dma-buf zero-copy**: rejected — several compositors silently drop
  input-popup dmabuf buffers with no protocol feedback; the SHM path is
  universally supported.

## Consequences

- Positive: no GPU dependency for panel rendering; no readback stall; one
  universally-supported present path; simpler dependency graph (no
  `flux-text-sys`, no dmabuf protocol).
- Trade-off: panel text quality now depends on the pure-Rust shaper/rasteriser
  rather than flux's FreeType/HarfBuzz GPU atlas; the `text_raster` stack was
  already proven by the tray badge path and shares the same libraries.
- Negative (accepted): software rasterisation is single-threaded and does not
  benefit from GPU parallelism; for a single candidate row this is well within
  frame budget.
