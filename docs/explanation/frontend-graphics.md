# Frontend Graphics

The host renders one floating UI: the **Panel**. Candidate composition,
engine/mode indicators, and voice status share the same input-popup
surface; the active producer is selected by the Panel Coordinator before
rendering starts.

Rendering uses flux's **software (CPU) canvas** plus
flux-text's CPU text rasterisation; there is no GPU device and no
window-system integration in the Panel path. A frame moves through these
stages in order:

1. The composition, indicator, or voice state the host currently holds.
2. Panel Coordinator ownership and anchor policy, which decide whether the
   Panel may be shown at all.
3. Panel sizing and candidate layout, measured once per candidate page.
4. CPU canvas draw commands, which paint the background and the selection
   highlight.
5. The text rasteriser, which composites shaped glyph coverage into the same
   RGBA8 framebuffer.
6. A host-managed shared-memory buffer, which receives the finished pixels.
7. Buffer attach, damage, and commit on the popup surface.

## Why CPU Rendering

The CPU canvas is a deliberate selection, not a fallback for missing GPU
support. The reasoning runs from the present path backward:

- Shared-memory presentation is the only present mechanism every compositor
  implements, so it is the only acceptable default for an input-popup surface;
  the destination of every frame is therefore host memory.
- The per-frame workload — two rounded-rects and a row of pre-shaped
  glyphs into a framebuffer on the order of a megabyte, redrawn on
  keystrokes — has no per-pixel effects for GPU parallelism to
  accelerate.
- A GPU renderer would still have to land the frame in host memory,
  paying a readback or dma-buf export that exceeds the entire CPU
  rasterisation cost, plus a GPU device lifecycle and worse latency
  variance.
- The glyph atlas and per-text-run texture machinery that a GPU renderer needs
  has no equivalent here: the CPU text path composites coverage directly, so
  there is no atlas to size, compact, or reclaim.

GPU rendering remains the right tool for components with a different
profile: the settings GUI renders through the GPU stack (Iris/Lens/Flux).
[ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md) records the
full decision, including the conditions under which the Panel would move
to the GPU.

## Rendering Boundaries

### Policy

Frontend policy lives in the host's event-loop layer, where the Panel
Coordinator and its caller decide:

- which UI owner may show;
- whether positioned UI has a trusted anchor;
- whether an anchor probe is needed;
- when a dirty candidate snapshot should be flushed.

This layer owns Wayland focus and input-method protocol facts. It does not draw.
It publishes only an already-approved draw request, so no renderer entry point
ever consults ownership state.

### Render

The platform panel owns the flux CPU canvas and the text rasteriser:

- the flux **CPU canvas** fills the panel background and selection
  highlight into a premultiplied RGBA8 framebuffer on the host;
- **flux-text** CPU text shaping and rasterisation shapes glyphs from
  host-resident coverage and composites them directly into that same RGBA8
  framebuffer, with the host resolving faces through fontconfig and exposing a
  family-class selector rather than individual faces;
- the drawing surface is a 64×32-pixel quantized grid cropped to the exact
  content extent through the viewport protocol when available; capacity shrinks
  with hysteresis after a wide page is no longer needed.

Two draw entry points exist — one for the Candidate Zone and one for the Status
Zone — and each records the canvas commands and the text draws for its own
content. There is **no GPU device, no offscreen GPU surface, no GPU→CPU
readback, no glyph texture upload** in this path.

### Present

The platform layer owns the triple-buffered shared-memory pool. A buffer is
selected before CPU drawing. The filled framebuffer is byte-swapped (RGBA8 →
the compositor's ARGB8888 layout) into that buffer's memory, then the buffer is
attached to the popup surface with a damage region and committed. If the
compositor has not released any buffer, the draw is skipped; the event loop
remains free to process input and later render the newest coalesced state.

No frame-callback pacing gate sits in this path. If every shared-memory buffer
is busy, the current frame is dropped and the latest dirty candidate snapshot is
retried after a later input or buffer-release event. Release tracking rides in
each buffer's own wayland user data: the busy flag is installed when the buffer
is created and cleared by the handler for the release event that arrived on that
buffer, so there is no proxy-pointer lookup and no possibility of a late
release clearing the wrong buffer's flag.

The panel extent is bounded (16384 logical px × 4096 px). Candidate rows wider
than the cap render a fitting prefix plus a `⋯` overflow marker; over-long
banner labels truncate with `…`. The engine supplies the candidate strings, so
this bound is a trust boundary against buggy or hostile engines, not a
cosmetic preference.

## Flux Dependency Boundary

The host's graphics dependencies are flux's **CPU canvas** and flux-text's
host-resident R8 coverage path. It does **not** depend on flux's GPU device,
surface, swapchain, or GPU readback APIs. The parts the host actually uses are:

- the CPU canvas lifecycle — create the canvas, begin and end one frame, and
  obtain a direct pointer into the RGBA8 framebuffer with no transfer bus and no
  fence;
- rounded-rectangle fills for the panel background and the selection highlight;
- the flux-text draw entry point, which shapes text and composites host-resident
  R8 glyph coverage into the CPU canvas.

Text is drawn by **flux-text** through the host's text rasteriser; glyphs
composite into the flux framebuffer directly. The tray badge uses the same
flux-text CPU stack; no pure-Rust shaping libraries
(`rustybuzz`/`fontdb`/`ab_glyph`) remain in the workspace.

Porting to another canvas backend would require replacing the panel's canvas
fill calls and the text compositing step. Ownership policy, anchor handling, key
routing, and engine state do not depend on flux.

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
- [Panel Rendering Blueprint](../architecture/panel-rendering.md) — the current
  render, present, and back-pressure implementation.
- [ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md) — CPU canvas render
  and host-managed SHM buffers.
- [ADR-0014](../adr/0014-canonical-panel-vocabulary.md) — canonical Panel
  terminology.
