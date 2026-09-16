# ADR-0014: Canonical Panel Vocabulary and Module Ontology

- **Status**: Accepted
- **Date**: 2026-05-30
- **Deciders**: Project maintainers
- **Refines**: [ADR-0005](0005-unified-panel-backend.md) (formalises the Phase 3 zone vocabulary it anticipated)

## Context

The floating IME UI is named inconsistently. A source survey finds **762**
occurrences of `popup` against **229** of `panel`, and the two words do not
partition cleanly:

- `popup` names both the **Wayland protocol surface**
  (`zwp_input_popup_surface_v2`) *and* the **internal render pipeline**
  (`PopupGeometry`, `PopupRow`, `popup_record`, `popup_present`,
  `src/ui/popup/`).
- `panel` names the **architecture/public layer** ([ADR-0005](0005-unified-panel-backend.md)'s
  "unified panel backend", `TypioPanelContent`, `TypioPanelStatus`, the public
  type `TypioWlCandidatePanel`).
- `TypioWlCandidatePanel` is **misnamed**: it carries status banners and voice
  indicators, not just candidates.
- `TypioWlTextUiBackend` is a thin wrapper that adds no concept over
  `TypioWlCandidatePanel` — a phased-migration artifact.
- There is a genuine **name collision**: `TypioTextLayout` (a shaped glyph run)
  versus the popup *layout* step (geometry). The word "layout" means two
  different things one call apart.
- `src/ui/renderer.c` does three unrelated jobs at once — the text engine, the
  per-glyph paint helper (`typio_flux_fill_layout`), and the shared GPU device
  (`typio_flux_device_get`).

The word `popup` is the root problem. In Wayland it is a **positioning role**
("a surface positioned relative to an anchor, transient, grab-associated") — it
describes *how a surface is placed*, not *what is shown*. Naming the UI after
it confuses the positioning mechanism with the thing positioned. The UI itself
aggregates candidates, preedit decoration, a status zone, and future
toolbar/waveform zones.

The right names are settled by **what each thing is**, not by what any other
project calls it. (That this vocabulary is also conventional across unrelated
IME stacks — IBus/macOS/Windows say "candidate window", Fcitx5 says "input
panel" — is at most corroboration that it reads naturally to newcomers; it is
not an authority this project follows. typio-linux is unrelated to those
stacks.)

This ADR is written under a **greenfield mandate**: define the ideal vocabulary
and object model with no backward-compatibility obligation, so naming and
boundaries are dictated by the concepts, not by the ~991 existing call sites.
Migration may be staged; the target is not negotiated against churn.

## Decision

Derive the object model from the subsystem's **real seams** — boundaries that
already exist objectively, each separating things with a different lifetime,
dependency direction, and test surface:

- **Seam A — platform-free vs platform-bound.** [ADR-0005](0005-unified-panel-backend.md)
  requires the content model to be free of Wayland/GPU types so it is testable
  without a display server. This is the load-bearing seam.
- **Seam B — data → geometry → pixels.** The three render-pipeline stages:
  pure data in, an immutable positioned snapshot, then draw commands.
- **Seam C — persistent vs per-frame.** Device, surface, and caches persist;
  geometry and frames are transient.
- **Seam D — surface lifecycle.** Acquire / present / recover is its own
  cohesive concern (the whole of [ADR-0006](0006-resilient-candidate-popup-present.md) /
  [ADR-0010](0010-non-blocking-candidate-popup-present.md) / [ADR-0013](0013-grow-only-popup-swapchain.md)).

### The object model

Umbrella term: **Panel** — a surface subdivided into regions, which is exactly
a multi-zone aggregator (the umbrella [ADR-0005](0005-unified-panel-backend.md) chose).

| Tier (seam) | Object / type | Responsibility | Platform |
|---|---|---|---|
| Model (A) | **`PanelContent`** | What to show — composed of **Zone**s: Candidate, Preedit, Status, (future) Toolbar | free · testable |
| Compose (B) | **Layout** → **`PanelGeometry`** | Pure function `Content + Theme + Scale → geometry` | free · testable |
| Compose (B) | **`Theme`** → **`Palette`** | Resolve appearance to colours/metrics | free |
| Render (B) | **`TextShaper`** → **`TextShape`** | String + font → colour-independent coverage mask + metrics ([ADR-0011](0011-colour-independent-coverage-glyphs.md)) | flux |
| Render (B) | **Painter** → **`Canvas`** → **`Frame`** | Walk geometry, emit draw commands, blit `TextShape`s | flux |
| Present (C,D) | **`PanelSurface`** | Own the input-popup `wl_surface` + swapchain + present/recover loop | wl + flux |
| Present (C) | **`RenderDevice`** | Shared GPU context | flux |
| Orchestrate | **`Panel`** | The controller the frontend owns: caches, `PanelSurface`, runs the pipeline, schedules, handles config/output changes | — |

**Persistent objects are exactly four** — `Panel`, `PanelSurface`,
`TextShaper`, `RenderDevice`. Everything else is a value (`PanelContent`,
`PanelGeometry`, `TextShape`, `Frame`, `Theme`/`Palette`) or a process (Layout,
Painter). A name earns its place only if it denotes one referent with a
distinct lifetime, dependency set, and test surface — no synonyms, no taxonomy
filler.

### What this fixes (real ambiguity, not cosmetics)

1. **`Layout` vs `TextLayout` collision** → the glyph run becomes **`TextShape`**;
   `Layout` denotes the geometry step alone.
2. **`renderer.c`'s three jobs** → split into `text_shaper.c` (shaping),
   `paint.c` (Painter), `device.c` (`RenderDevice`).
3. **`popup` retired as a concept noun** → it survives only as the literal
   protocol object, owned *inside* `PanelSurface`.
4. **`TypioWlTextUiBackend` + `TypioWlCandidatePanel`** → one `Panel`. (The
   flux-vs-stub build split is a `PanelSurface` implementation choice — [ADR-0005](0005-unified-panel-backend.md)
   Phase 4's `SurfaceProvider` — not a second public type.)
5. **`Zone` becomes grounded**: a zone is a concrete triple — a `PanelContent`
   fragment, its `PanelGeometry` fragment, and the Painter that draws it.
   Realises [ADR-0005](0005-unified-panel-backend.md) Phase 3.

### Greenfield module layout

```text
src/ui/panel/
  panel.{c,h}        Panel — orchestrator        (merges backend.c + popup/panel.c)
  content.h          PanelContent + Zone model   (moves from platform/panel.h)   [pure]
  layout.{c,h}       Layout: Content → PanelGeometry (LRU cache)                  [pure]
  theme.{c,h}        Theme → Palette                                              [pure]
  paint.{c,h}        Painter: PanelGeometry → draw commands on a Canvas          [flux]
  text_shaper.{c,h}  TextShaper → TextShape       (was renderer.c, shaping part)  [flux]
  device.{c,h}       RenderDevice                 (was renderer.c, device part)   [flux]
  surface.{c,h}      PanelSurface: input-popup surface + swapchain + present  [wl+flux]
  surface_stub.c     No-op PanelSurface when HAVE_FLUX is off
```

Symbol conventions: public `typio_panel_*`, `typio_panel_content_*`,
`typio_panel_surface_*`, `typio_text_shaper_*`; types `TypioPanel`,
`TypioPanelContent`, `TypioPanelGeometry`, `TypioPanelZone`, `TypioPanelSurface`,
`TypioPanelTheme`, `TypioTextShape`, `TypioRenderDevice`; internal
`panel_layout_*`, `panel_paint_*`.

### Old → canonical map

| Current | Canonical |
|---|---|
| `TypioWlTextUiBackend`, `TypioWlCandidatePanel` | `TypioPanel` |
| `typio_wl_text_ui_backend_*`, `typio_wl_candidate_panel_*` | `typio_panel_*` |
| `TypioPanelContent` (+ `typio_panel_content_init`) | unchanged (already canonical) |
| `PopupGeometry`, `PopupRow` | `TypioPanelGeometry`, `PanelRow` |
| `PopupRenderCtx`, `PopupLayoutEntry`, `PopupConfig` | `PanelLayoutCache`, `PanelLayoutEntry`, `PanelConfig` |
| `popup_geometry_compute` | `panel_layout_compute` |
| `popup_record` / `PopupPaintTarget` | `panel_paint` / `PanelPaintTarget` |
| `TypioTextEngine`, `TypioTextLayout` | `TypioTextShaper`, `TypioTextShape` |
| `typio_flux_fill_layout` | `panel_paint_text_shape` (Painter blit) |
| `typio_flux_device_get` | `typio_render_device_get` |
| `popup_present`, `ensure_fx_surface` | `panel_surface_present`, `panel_surface_ensure` |
| `Popup*Theme*`, palette helpers | `PanelTheme*` |
| `src/ui/popup/`, `src/ui/renderer.c`, `src/platform/panel.h` | `src/ui/panel/`, split into `text_shaper.c`+`device.c`, `…/panel/content.h` |
| `zwp_input_popup_surface_v2` handles | unchanged (protocol-owned; the lone surviving "popup") |

## Alternatives considered

**Umbrella term**

- **`Overlay`.** Most accurate about *floating over the application* and GPU
  compositing. Rejected: it says nothing about grouped multi-zone content and
  is vague about being an interactive IME element.
- **`Candidate window`.** The cross-IME-standard term, instantly recognised.
  Rejected as the *type* name: the surface now also carries status/voice (not
  candidate-only), and "window" is badly overloaded in Wayland. Retained as a
  **user-facing prose synonym** in documentation and as the **Candidate Zone**
  name.
- **Keep `popup` as the umbrella, rename `panel` → `popup`.** Rejected: `popup`
  is a positioning role; this names the UI after the mechanism that places it.

**Structure**

- **Document-only convention, no rename.** Rejected under the greenfield
  mandate: it leaves 762 live uses of an overloaded word and the
  `Layout`/`TextLayout` collision intact. A convention the code contradicts rots.
- **Keep `TypioWlTextUiBackend` and `TypioWlCandidatePanel` as two types.**
  Rejected: the boundary is a migration accident, not a concept. The seam worth
  keeping is `PanelSurface` (flux vs stub), not a second public controller type.
- **Keep `renderer.c` whole.** Rejected: it conflates three lifetimes (shaper
  cache, per-frame paint, persistent device) behind one filename — Seam C drawn
  through the middle of a file.

## Consequences

- **Positive**: one word per concept. `popup` means exactly the protocol
  surface; `Panel` is the multi-zone umbrella; `Layout` and `TextShape` no
  longer collide. The pure core (content / layout / theme / paint) is cleanly
  separated from the platform tier (surface / device / shaper), satisfying
  [ADR-0005](0005-unified-panel-backend.md)'s testability rule structurally
  rather than by discipline.
- **Positive**: unblocks [ADR-0005](0005-unified-panel-backend.md) Phase 3
  (zones are first-class) and Phase 4 (a layer-shell provider is just another
  `PanelSurface`) with no further renaming.
- **Trade-off**: a large diff — renames across ~991 sites plus splitting
  `renderer.c` into three files and promoting `PanelSurface` to a real object.
  It must land as one atomic sweep or a tracked series; the v0.0.x pre-1.0
  window (public API unfrozen) is the cheapest time. Accepted explicitly: the
  rework is worth a correct foundation.
- **Trade-off**: external tooling referencing `typio_wl_candidate_panel_*`
  breaks. Accepted — the greenfield mandate drops backward compatibility, and no
  stable consumer exists pre-1.0.
- **Negative (accepted)**: `popup` cannot be eliminated entirely — it is baked
  into the Wayland protocol we do not own (`zwp_input_popup_surface_v2`). It is
  bounded by convention to that protocol object and lives only inside
  `PanelSurface`.
- **Doc impact**: `docs/explanation/frontend-graphics.md` carries the glossary
  and is aligned to these terms; the ADR index notes this ADR refines
  [ADR-0005](0005-unified-panel-backend.md)'s vocabulary.
