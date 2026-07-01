# Panel Appearance Development Notes

Rendering pipeline for the candidate Panel: offscreen GPU rendering, SHM
presentation, font loading, theme resolution, and cache invalidation.

---

## GPU render and SHM present pipeline

The Panel renders with flux (Vulkan) into an offscreen image and attaches the
result to its `zwp_input_popup_surface_v2` `wl_surface` through host-managed
`wl_shm` buffers. There is no Vulkan device, surface, or dma-buf in the panel
path; see
[ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md).

`FluxPanel` (`crates/typio-host/src/panel.rs`) drives the render pipeline:

- `FluxPanel::new_from_surface()` creates a flux device without WSI extensions,
  an offscreen `flux_surface` (`vk_surface_khr = NULL`), a `flux_canvas`, a
  `flux_text` context, and a small arena.
- `ensure_candidate_size()` / `ensure_banner_size()` grow the offscreen image in
  quantised physical pixels. When `wp_viewporter` is available,
  `wp_viewport.set_source` / `set_destination` crop the oversized image to the
  exact logical panel size ([ADR-0013](../adr/0013-grow-only-popup-swapchain.md),
  adapted by ADR-0040).
- `draw_candidates()` and `draw_status_banner()` record one frame:
  `flux_surface_begin_frame` → `flux_canvas_begin` → paint → `flux_canvas_end`
  → `flux_frame_submit` → `flux_frame_present` (offscreen no-op) →
  `flux_surface_read_pixels`.
- `present_shm()` copies the readback into a free SHM buffer, sets
  `wl_surface.set_buffer_scale`, attaches the `wl_buffer`, damages the full
  buffer, and commits the popup surface.

Text is drawn from a **shared, colour-independent glyph atlas**
([ADR-0012](../adr/0012-glyph-atlas-shared-texture.md)). Each glyph is rasterised
by FreeType once into a single long-lived R8 *coverage* texture;
`typio_text_shape_fill` then draws one tinted quad per glyph sampling that
sub-rect, so the colour (normal / muted / selection) is a **draw-time tint**
([ADR-0011](../adr/0011-colour-independent-coverage-glyphs.md)) and no per-text
GPU upload happens during candidate navigation. Solid fills (background, border,
selection) use premultiplied RGBA via `flux_color_rgba_premul`.

The glyph atlas reclaims itself — a wholesale rebuild when the hash load exceeds
75 % or the shelf packer exhausts the texture
([ADR-0020](../adr/0020-atlas-reclamation-and-glyph-layer-modularization.md)),
so neither lookup degradation nor texture saturation accumulates during extended
CJK input sessions.

---

## Present pacing and buffer back-pressure

The panel render path runs synchronously on the single-threaded event loop. To
keep the loop responsive when a compositor stops releasing buffers, the host
owns the SHM pool and never waits for compositor release.

- `flux_surface_begin_frame` is called with `PANEL_FRAME_TIMEOUT_NS` (200 ms)
  instead of an infinite wait.
- `ShmBufferPool::acquire()` returns `None` if every SHM buffer is busy. The
  panel drops that frame and lets the next dirty tick render the newest
  candidate state.
- `wl_surface.frame` callbacks pace healthy compositors. If a callback goes
  missing, `panel_present_gate` waits only for the soft limit and then allows a
  timer-paced submit. An extended missing-callback episode logs a warning.

A skipped frame never freezes key handling: input events queue on the Wayland
fd while the compositor catches up, so navigation stays correct even while the
on-screen highlight is briefly behind.

---

## Font selection and sizing

The candidate panel renders text on the CPU via `TextRaster`
(`crates/typio-host/src/text_raster.rs`): `rustybuzz` (shaping), `fontdb`
(per-codepoint face discovery — `fontdb` reads the system `fonts.conf`, so the
family list mirrors the desktop's Fontconfig configuration), and `ab_glyph`
(outline rasterisation). The legacy FreeType/HarfBuzz/Fontconfig `text_shaper.c`
path was retired by [ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md).

### Primary family + per-codepoint fallback

`TextRaster::face_score` assigns every covering face a sort key. The
user-configured family (`display.font_family`) is the **highest-priority tier**
(tier 0): it wins for every codepoint it covers. When it does not cover a
codepoint (e.g. a Latin family meeting a CJK character), selection falls through
to the built-in fallback lists (`CJK_SANS_FAMILIES`, `UI_SANS_FAMILIES`) and
finally to any system face that covers the codepoint, with upright faces and
weights near Regular preferred (CJK prefers ~Medium for visual balance at small
sizes). This mirrors the fontconfig "first font then fallback" contract without
a native `libfontconfig` dependency.

Changing `display.font_family` (config reload) calls `set_preferred_family`,
which flushes the per-codepoint coverage cache and all loaded faces so the new
family is resolved from scratch.

### Sizing

`display.font_size` (points, 6–72, default 11) drives the candidate, index-number
and banner sizes via `PanelFontConfig` (`crates/typio-host/src/app/font_config.rs`),
converted to logical pixels at 96/72 px/pt. The HiDPI scale is applied
separately at draw time. `PanelFontConfig` is snapshotted at startup and on every
config reload, then pushed onto the panel via `FluxPanel::set_font_config`.

---

## Font and glyph caches

`TextRaster` keeps two caches, both flushed by `set_preferred_family`:

- `by_face` / `entries`: loaded `(bytes, index, ab_glyph FontVec)` per font face.
- `cover`: per-codepoint → covering face index (or `None`).

Glyphs are rasterised on demand by `ab_glyph` directly into the panel's
premultiplied-RGBA8 scratch framebuffer each frame — there is no shared GPU
glyph atlas in the CPU-canvas path (contrast the retired
[ADR-0011](../adr/0011-colour-independent-coverage-glyphs.md) coverage-texture
model, which belonged to the Vulkan renderer). Colour is a straight RGB
parameter on `TextRaster::draw`.

---

## Theme resolution

The Panel supports three modes:

| Mode | Behaviour |
|---|---|
| `auto` | Detects desktop dark/light from GTK_THEME, gtk-3.0/4.0 settings.ini, or KDE kdeglobals |
| `light` | Built-in light palette |
| `dark` | Built-in dark palette |

The resolved palette is cached with a 5-second TTL to avoid repeated filesystem reads during rapid render cycles.

Users can override individual channels per mode via `display.colors.light.*` and `display.colors.dark.*` in the config file. The `panel_config_build_palette` function applies these overrides on top of the built-in base palette.

### When adding a new colour channel

1. Add the fields to `TypioPanelPalette` in `theme.h`
2. Add defaults to `palette_light` and `palette_dark` in `theme.c`
3. Add parsing support in `panel_config_load` (`LOAD_VARIANT` macro)
4. Add override application in `panel_config_build_palette`
5. Use the new colour in `paint.c`
6. Update user-facing configuration documentation

---

## Layout cache invalidation

`PanelRenderCtx` maintains an LRU layout cache keyed by candidate label + text + font description (label and main). Colour is not part of the key — glyphs are colour-independent R8 coverage ([ADR-0011](../adr/0011-colour-independent-coverage-glyphs.md)), so the selected and unselected states of a row share one cache entry.

Changing the font weight, size, or family produces a different cache key. The cache does **not** survive `panel_render_ctx_invalidate`, which happens on theme or config changes.
