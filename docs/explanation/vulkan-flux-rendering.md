# Vulkan and Flux Rendering

This document describes how typio renders its Panel UI through
[flux](../../flux), a C23 Vulkan-first canvas library, and the performance
optimizations that keep an IME popup responsive under sustained input.

The pipeline stages and the backend-independence argument are covered in
[Frontend Graphics](frontend-graphics.md). This document focuses on the
**rendering technology itself** — what flux provides, how the host uses it,
and the specific optimizations that prevent stuttering.

## Flux — Vulkan Canvas Library

[flux](../../flux) is a self-contained C23 graphics library that wraps Vulkan
1.3 into a small immediate-mode 2-D canvas API. typio consumes it as a
meson subproject and links it statically. The host never calls Vulkan directly.

Core flux concepts used by the host:

| Concept | flux type | Role |
|---------|-----------|------|
| GPU context | `flux_device` | Process-wide shared device; created once at startup |
| Offscreen target | `flux_surface` | GPU image used as the Panel render target |
| Frame lifecycle | `flux_surface_begin_frame` → `flux_frame_submit` → `flux_surface_read_pixels` | Record, submit, and read back the rendered frame |
| Recording target | `flux_canvas` | Immediate-mode: clear, fill rect, blit coverage |
| GPU image | `flux_image` | Refcounted texture; atlas is one `flux_image` |

Frame lifecycle in `FluxPanel`:

```c
flux_surface_begin_frame(surface, &(flux_frame_begin_desc){ .timeout_ns = ... }, &frame);
flux_canvas_begin(canvas, frame, &clear_color);
panel_record(panel, canvas, geometry);   /* pure recorder */
flux_canvas_end(canvas);
flux_frame_submit(frame);
flux_frame_present(frame);               /* offscreen no-op */
flux_surface_read_pixels(surface, pixels, len);
```

The host's drawing reduces to three primitives: **clear**, **fill rect**,
**tinted coverage blit**. Nothing in the panel requires paths, custom shaders,
blend exotica, or retained scene graphs.

## Text Rendering

### Shaping stack

| Component | Role |
|-----------|------|
| Fontconfig | Font discovery (`FcFontMatch`, `FcFontSort`) |
| HarfBuzz | Text shaping (cluster → glyph mapping) |
| FreeType | Glyph rasterisation (outline → R8 coverage bitmap) |

The primary font is resolved from `display.font_family` (default `"Sans"`). When
a glyph resolves to `.notdef` (glyph ID 0), the shaper performs **per-glyph
fallback** via `FcFontSort` + `FT_Get_Char_Index` verification, trying up to
four fallback fonts per text run
([ADR-0016](../adr/0016-per-glyph-font-fallback.md)).

### Colour-independent coverage

Glyphs are rasterised into **R8 coverage** textures — single-channel alpha masks
that carry no colour information. Colour is applied at draw time as a
premultiplied `flux_color` tint via
`flux_canvas_draw_image_coverage(_sub)`:

```c
flux_canvas_draw_image_coverage_sub(canvas, atlas, dst_rect, src_uv, tint);
```

One glyph serves every colour variant (normal, muted, selection highlight,
preedit). Selecting a candidate row only adds a highlight rectangle and changes
the tint — zero GPU upload work
([ADR-0011](../adr/0011-colour-independent-coverage-glyphs.md)).

### Shared glyph atlas

All glyphs share one persistent 2048×2048 R8 atlas texture (~4 MiB GPU memory),
keyed `(font_id, glyph_id)` in an open-addressed hash table. A skyline shelf
allocator packs new glyphs into sub-rectangles
([ADR-0012](../adr/0012-glyph-atlas-shared-texture.md)).

**Why this matters for responsiveness:** before the atlas, each text run
rasterised into its own texture and uploaded it synchronously
(`vkWaitForFences` on the IME event loop). RIME candidate navigation pages the
list on every keypress, so every page triggered ~20 blocking fence waits. With
the atlas, a warmed session uploads **nothing** — CJK glyphs are shared across
every candidate and page.

### LRU layout cache

`panel_geometry_compute()` is a pure function from content + config to
`PanelGeometry`. Results are memoised in an LRU cache (capacity 128) keyed by
text + font. Paging through candidates does not re-shape text already seen.

### Atlas reclamation

Over long CJK sessions two resources accumulate dead weight: the lookup hash
table lengthens its linear-probe chains as LRU-evicted layouts leave entries
behind, and — the binding constraint — the **texture** saturates, because the
shelf packer only ever advances. At a 16 px default the 2048² image holds
~9–10 K glyphs (~3–4 K on a HiDPI output), so a daily CJK session fills it well
before the hash table reaches its 75 % threshold (98 304 of 131 072 slots).

`glyph_atlas_reclaim()` handles both with one wholesale rebuild — image, slot
table, packer cursor, and live count are all reset — triggered when the hash
load crosses 75 % **or** the packer reports the texture is full. The next draw
re-rasterises the visible page lazily into the fresh atlas, so the cost is
bounded by the page size, not session history. The check is a single comparison
at the top of every `panel_render()`, behind a device-idle fence so tearing the
in-use image down is safe ([ADR-0020](../adr/0020-atlas-reclamation-and-glyph-layer-modularization.md),
superseding [ADR-0019](../adr/0019-atlas-hash-compaction.md)).

Without it the texture filled and new glyphs rendered blank permanently — the
original "panel goes stale after a while" bug.

## Offscreen Image and SHM Presentation

### Grow-only, size-quantised

The popup width varies with candidate string length. Previously, every width
change rebuilt the swapchain (`vkDeviceWaitIdle` + WSI roundtrips), stalling
the single-threaded IME loop.

The current fix keeps the same grow-only idea on the offscreen image: physical
buffer sizes are quantised and **grow only**. Shrinks and sub-quantum
widenings reuse the existing image. `wp_viewport.set_source` crops the
oversized buffer to the exact content rect. After a short warm-up (the widest
candidate row seen), `flux_surface_resize` is never called again during
steady-state paging ([ADR-0013](../adr/0013-grow-only-popup-swapchain.md),
adapted by [ADR-0040](../adr/0040-offscreen-render-shm-buffers.md)).

### Host-managed SHM buffers

The panel no longer presents through Vulkan WSI. It renders to an offscreen
flux surface (`vk_surface_khr = NULL`), reads the pixels back, and attaches
them through a double-buffered `wl_shm` pool. `wl_buffer.release` is delivered
to the host event queue, so the daemon controls buffer reuse directly. If all
buffers are busy, the frame is dropped and the next dirty tick renders the
newest candidate state.

### Frame callback pacing

`wl_surface.frame` remains a pacing hint. A healthy compositor sends `done` at
refresh rate; a missing callback only delays until the soft gate expires. The
daemon then submits the latest coalesced candidates and logs an extended
missing-callback episode for diagnosis.

The two protections are complementary:

| Concern | Protection |
|---------|------------|
| Compositor does not release buffers | Drop frame when SHM pool is busy (ADR-0040) |
| Steady-state commit throttle | `wl_surface.frame` soft gate |
| Per-page resize | Grow-only quantised offscreen image (ADR-0013/0040) |

## Font Cache Management

Over long sessions, Fontconfig's internal caches grow unbounded (repeated
`FcInit()` without `FcFini()`). The host exports
`typio_flux_engine_purge_font_caches()` and calls it on every config reload,
clearing the font-object cache, font-file cache, fallback-font cache, and
Fontconfig internals. This bounds memory without requiring a daemon restart
([ADR-0009](../adr/0009-long-term-performance-optimizations.md)).

## Event-Loop Integration

All rendering runs synchronously on the single-threaded IME event loop
([ADR-0004](../adr/0004-event-loop-scheduling-and-watchdog.md)). There is no
dedicated render thread. Input callbacks set `popup_update_pending`; the render
is flushed once per loop iteration in the `POPUP_UPDATE` watchdog stage. This
design avoids cross-thread Wayland/flux state access and protocol-sequencing
risk.

A consequence of this architecture: a frozen panel never corrupts committed
text. Key events queue on the Wayland fd and are processed in order regardless
of what the renderer is doing. The visible highlight may lag, but the committed
selection stays correct.

## Performance Summary

The optimizations form a layered defence against IME-specific stuttering
patterns. Each addresses a distinct measured cause:

| Layer | Problem | Fix | ADR |
|-------|---------|-----|-----|
| Text shaping | Per-text-run synchronous texture upload | Shared R8 glyph atlas, upload once | 0012 |
| Text colour | Per-colour glyph duplication | Coverage + draw-time tint | 0011 |
| Atlas reclaim | Texture saturates / hash table degrades over a session | Wholesale rebuild on packer exhaustion or 75 % load | 0020 |
| Fallback fonts | `FcFontSort` re-run per layout under LRU churn | Per-codepoint resolution memo | 0020 |
| Offscreen image | Resize on every width change | Grow-only quantised buffers | 0013/0040 |
| Present | WSI present blocks on compositor buffer release | Host-managed SHM buffers; drop when busy | 0040 |
| Pacing | Missing frame callbacks freeze redraws | Soft frame-callback gate | 0036/0040 |
| Fonts | Fontconfig unbounded cache growth | Purge on config reload | 0009 |
| Snapshot | Full deep-copy on selection-only changes | Content-equality fast-path | 0009 |

## See also

- [Frontend Graphics](frontend-graphics.md) — pipeline architecture and backend-independence argument.
- [ADR-0014 — Canonical Panel Vocabulary](../adr/0014-canonical-panel-vocabulary.md) — the object model and naming.
- [ADR-0016 — Per-glyph Font Fallback](../adr/0016-per-glyph-font-fallback.md) — mixed-script rendering.
