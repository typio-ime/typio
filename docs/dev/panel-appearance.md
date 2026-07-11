# Panel Appearance Development Notes

Rendering pipeline for the candidate Panel: CPU canvas rendering, SHM
presentation, font loading, theme resolution, and cache invalidation.

---

## CPU render and SHM present pipeline

The Panel renders entirely on the CPU — flux's software canvas for fills and
flux-text for glyphs (FreeType/HarfBuzz/Fontconfig via flux-text-sys) — and
attaches the result to its
`zwp_input_popup_surface_v2` `wl_surface` through host-managed `wl_shm`
buffers. There is no Vulkan device, surface, swapchain, dma-buf, or GPU readback
in the panel path; see [ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md).

`FluxPanel` (`crates/typio-host-platform/src/panel.rs`) drives the render pipeline:

- `FluxPanel::new_from_surface()` creates a flux **CPU canvas**
  (`flux_canvas_create_cpu`), the text rasteriser (`TextRaster`), and the
  grow-only scratch framebuffer.
- `ensure_candidate_size()` / `ensure_banner_size()` grow the framebuffer in
  quantised physical pixels. When `wp_viewporter` is available,
  `wp_viewport.set_source` / `set_destination` crop the oversized image to the
  exact logical panel size ([ADR-0013](../adr/0013-grow-only-popup-swapchain.md),
  adapted by ADR-0040).
- `draw_candidates()` and `draw_status_banner()` record one frame:
  `flux_canvas_cpu_begin` → `flux_canvas_fill_rrect` (background + highlight)
  → `TextRaster::draw` composites glyphs into the framebuffer →
  `flux_canvas_cpu_end`.
- `present_shm()` byte-swaps the framebuffer (RGBA8 → ARGB8888) into a free SHM
  buffer, sets `wl_surface.set_buffer_scale`, attaches the `wl_buffer`, damages
  the full buffer, and commits the popup surface.

Text is shaped and rasterised by flux-text (via `flux-text-sys`:
FreeType/HarfBuzz/fontconfig) directly into the premultiplied RGBA8 framebuffer.
Solid fills (background,
border, selection) use premultiplied RGBA via `flux_color_rgba_premul`. There is
**no shared GPU glyph atlas** in the CPU-canvas path; the retired coverage-texture
model ([ADR-0011](../adr/0011-colour-independent-coverage-glyphs.md)) belonged to
the Vulkan renderer and is no longer in effect.

---

## Present pacing and buffer back-pressure

The panel render path runs synchronously on the single-threaded event loop. To
keep the loop responsive when a compositor stops releasing buffers, the host
owns the SHM pool and never waits for compositor release.

- `ShmBufferPool::acquire()` returns `None` if every SHM buffer is busy. The
  panel drops that frame and lets the next dirty reactor step render the newest
  candidate state.
- There is no `wl_surface.frame` pacing gate. SHM buffer release is the only
  compositor back-pressure signal on the candidate path.

A skipped frame never freezes key handling: input events queue on the Wayland
fd while the compositor catches up, so navigation stays correct even while the
on-screen highlight is briefly behind.

---

## Font selection and sizing

The candidate panel renders text on the CPU via `TextRaster`
(`crates/typio-host-platform/src/text_raster.rs`): a thin FFI wrapper over **flux-text**
(`flux-text-sys`: FreeType + HarfBuzz + Fontconfig + FriBidi), exposing
`flux_text_create` / `flux_text_measure` / `flux_text_draw`. The former
`ab_glyph`/`rustybuzz`/`fontdb` software rasteriser was retired by
[ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md); those crates now serve
only `icon_badge.rs` (tray badges, behind `feature = "systray"`).

### Family selection

Font selection is delegated to flux-text's fontconfig backend: the configured
family (`display.font_family`) is the preferred family, and fontconfig resolves
per-codepoint fallback (e.g. a Latin family meeting a CJK character) through the
desktop's `fonts.conf`. `TextRaster::set_preferred_family` is a **no-op**
retained only for source compatibility with the former `ab_glyph` rasteriser —
flux-text picks up the preferred family at `flux_text_create` time, and a family
change is applied by recreating the text context on config reload.

### Sizing

`display.font_size` (points, 6–72, default 11) drives the candidate, index-number
and banner sizes via `PanelFontConfig` (`crates/typio-host/src/app/font_config.rs`),
converted to logical pixels at 96/72 px/pt. The HiDPI scale is applied
separately at draw time. `PanelFontConfig` is snapshotted at startup and on every
config reload, then pushed onto the panel via `FluxPanel::set_font_config`.

---

## Font and glyph caches

`TextRaster` holds a single `flux_text*` context; face and glyph caches live
inside flux-text (FreeType/HarfBuzz/fontconfig), so the Rust wrapper keeps none.
`set_preferred_family` is a no-op (see above).

Glyphs are rasterised on demand by flux-text into the panel's premultiplied
RGBA8 framebuffer each frame via the host-coverage path (ADR-0019) — there is no
shared GPU glyph atlas in the CPU-canvas path (contrast the retired
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

`PanelRenderCtx` maintains an LRU layout cache keyed by candidate label + text +
font description (label and main). Changing the font weight, size, or family
produces a different cache key. The cache does **not** survive
`panel_render_ctx_invalidate`, which happens on theme or config changes.
