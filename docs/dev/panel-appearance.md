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

## Font loading and variable fonts

### Font description parsing

`parse_font_desc` in `text_shaper.c` understands descriptions such as:

```
"Noto Sans SemiBold 16"
```

It extracts:
- family: `"Noto Sans"`
- weight: `600` (SemiBold)
- size: `16`

### Font file selection via FontConfig

`match_font_file` asks FontConfig for a file matching `(family, weight)`. For traditional static fonts this returns different files (`NotoSans-Regular.ttf`, `NotoSans-Bold.ttf`, etc.).

### The variable-font trap

Modern systems often ship **variable fonts** — a single `.ttf` file (e.g. `NotoSans-VariableFont_wdth,wght.ttf`) that contains every weight from 100 to 900. FontConfig returns this one file for *all* weights, but FreeType loads it as the **default instance** (usually Regular, `wght = 400`).

If you do not set the variable axis, asking for SemiBold (600) or Bold (700) renders identically to Regular (400).

**Fix:** after `FT_New_Face`, detect a variable font via `FT_Get_MM_Var`, find the `wght` axis, and set it with `FT_Set_Var_Design_Coordinates`.

Call this **before** `FT_Set_Pixel_Sizes`.

---

## Font object caching

`font_obj_cache` stores `(path, size, weight)` → `(FT_Face, hb_font_t)`. The cache key **must include weight** — variable fonts mutate the face's `wght` axis in place, so omitting weight would alias Medium and SemiBold to the same `FT_Face`.

`TypioTextShape` borrows the cached `FT_Face`. Glyphs are rasterised once via `FT_Load_Glyph` into the atlas on first sight; subsequent draws are atlas hits with no FreeType call. Shapes must not outlive their font cache entry — `panel_render_ctx_invalidate` frees all shapes before eviction (draining the retire ring behind a device fence first).

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
