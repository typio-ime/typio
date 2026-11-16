---
id: ADR-0063
title: "Optics v0.0.80 adoption boundary"
status: accepted
date: 2026-11-16
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0063: Optics v0.0.80 adoption boundary

- **Status**: Accepted
- **Date**: 2026-11-16
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: None

---

## Context

Optics advanced from `v0.0.79` to `v0.0.80`. Unlike the previous release, this
interval is a source-breaking API release, not just a distribution move:

- **One-way library graph and the Aperture application shell (Optics
  ADR-0166).** Iris no longer depends on Lens or owns the lens-flavoured
  application entry point. `iris_app_run` is replaced by
  `aperture_app_run`; the new `aperture` library binds a Lens UI to an Iris
  window. In the facade, `app` (Aperture) becomes the application feature and
  `platform` (Iris) no longer implies `ui` (Lens). `optics::platform` therefore
  exposes window, cursor, dialog, and theme APIs only; `Application`, `Config`,
  and `RunError` now live in `optics::app`.
- **One canvas creation path (Optics ADR-0166).** The dedicated
  `flux_canvas_create_cpu`, `flux_canvas_create_cpu_aa`, and
  `flux_canvas_create_cpu_ss` factories are removed. A CPU canvas is created by
  `flux_canvas_create` with a `flux_canvas_desc` whose `backend` is
  `FLUX_CANVAS_BACKEND_CPU` and whose `width`/`height` are explicit. Both
  backends run unconditionally at 1× resolution with continuous analytic
  coverage; the 4× MSAA, supersampling, stencil-then-cover, and tessellator
  paths are retired.
- **Single opts descriptors (Optics ADR-0165).** Multi-parameter operation
  APIs moved to one descriptor argument. The Typio-reachable changes are
  `glyph_measure(glyph_ctx *, const glyph_measure_opts *)`,
  `glyph_draw(glyph_ctx *, const glyph_draw_opts *)`, and
  `flux_canvas_draw_geometry(flux_canvas *, const
  flux_canvas_draw_geometry_opts *)`. `glyph_draw_outlined` is folded into
  `glyph_draw_opts` through `outline_color` and `outline_width`.
- **C23 baseline (Optics ADR-0165).** Every `typedef enum` now carries an
  explicit integer underlying type. The Typio `-sys` bindings are generated
  from those headers, so enum-typed descriptors change representation; the
  exact-version `pkg-config` probes make a mixed native/binding tree fail
  closed.
- **Repository-root Cargo workspace and umbrella flags (Optics ADR-0163,
  ADR-0164).** The nested `bindings/` workspace moved to the Optics repository
  root. `<optics/optics.h>` now keys module availability on
  `OPTICS_HAVE_<MODULE>` distribution flags. Neither changes a Typio call
  site: Typio patches the facade by path and uses granular headers beneath it.

Typio's two consumers are affected differently:

- The **daemon and host platform** render the Panel and tray badges through
  raw `optics::canvas::sys` and `optics::typography::sys`. They are affected by
  the canvas-factory and glyph-descriptor migrations, not by the application
  shell.
- **`typio-settings`** is a Lens application. It previously enabled `platform`
  and imported `Application`/`Config` plus Lens chrome types from
  `optics::platform`. With the one-way graph, Iris no longer re-exports Lens or
  provides the run entry point.

## Decision

1. **Move the pin atomically to Optics `v0.0.80`.** Update the CI
   `OPTICS_PINNED_REF` to commit
   `e4304a56d1cbf89a3381b0ff0061da6a04f0cbf8` (tag `v0.0.80`) and regenerate
   the canonical `Cargo.lock` when the change is promoted to `main`.

2. **Enable `app` for `typio-settings`.** Replace the facade feature
   `platform` with `app`, which pulls in Aperture, Lens, Iris, Glyph, and Flux.
   Import `Application`, `Config`, and `RunError` from `optics::app`; import
   Lens chrome handles (`Align`, `Frame`, `LayoutOpts`, `TextBuf`, …) from the
   same Aperture re-export. The settings binary remains Lens chrome; it still
   registers no host canvas callback.

3. **Create every CPU canvas through `flux_canvas_create`.** The Panel's
   `ensure_canvas` and the tray badge rasteriser build a
   `flux_canvas_desc` with `backend = FLUX_CANVAS_BACKEND_CPU`, explicit
   `width`/`height`, and the existing content `scale`; they never request a
   GPU surface. The Panel's whole-extent clear, SHM back-pressure, and
   quantized extent policy are unchanged.

4. **Adopt the glyph operation descriptors.** `TextRaster::measure`,
   `TextRaster::draw`, and the tray badge path pass `glyph_measure_opts` /
   `glyph_draw_opts`. Badge outlines use `glyph_draw_opts.outline_color` and
   `outline_width` instead of the removed `glyph_draw_outlined` symbol, with
   the same 1 px normalised contour.

5. **Pass the new canvas descriptor to `flux_canvas_draw_geometry`.** The
   Panel's background and selection fills wrap the existing
   `flux_geometry`/`flux_brush` pair in `flux_canvas_draw_geometry_opts`.

6. **Extend rpath derivation to Aperture.** The shared Typio build helper and
   the local patch template add `APERTURE_BUILD_DIR`, `APERTURE_SOURCE_DIR`,
   and the `libs/aperture` directory, so `typio-settings` links and runs against
   the sibling Meson tree without `LD_LIBRARY_PATH`.

7. **Decline the new GPU paths and unreachable layers.** The GPU tiled
   rasterizer, dynamic image usage descriptors, Reticle, Transit/motion, Prism,
   and Vista remain unadopted because no Typio path presents to a GPU surface
   or reaches those libraries.

## Rationale

**Why `app` rather than a hand-built feature set.** `app` is the facade's
declared composition for running a Lens UI in an Iris window, and Aperture is
now the only public entry point that does so. Enabling `ui` + `platform`
separately would still not provide `Application`, because the shell lives in
Aperture; enabling `app` keeps the settings feature intent explicit instead of
reconstructing the upstream graph by hand.

**Why the CPU backend is explicit rather than inferred.** `AUTO` selects GPU
when a surface is present and CPU otherwise. The Panel has no surface, so
`AUTO` would happen to work, but the invariant that no GPU object exists in the
Panel path is stronger when the descriptor states `FLUX_CANVAS_BACKEND_CPU`.
The migration changes the factory, not the renderer: the Panel remains a
device-less CPU canvas whose pixels are byte-swapped into `wl_shm`.

**Why the descriptor migrations are mechanical.** Optics removed the alternate
entry points rather than adding behaviour behind them. The glyph outline now
travels in the draw descriptor, and the canvas geometry call takes the same
geometry and brush behind one struct. Adopting them is a call-shape change;
the Panel's output, the badge's outline, and the settings UI are unchanged.

**Why no behaviour change is expected from C23.** Typio does not construct
enum-typed descriptors with hand-written integer widths. Bindgen generates the
typed enums from the same headers the native libraries are built from, and the
`-sys` build scripts require an exact package-version match, so an old system
install fails at build time rather than at runtime.

**Why the pin and local paths move together.** The `-sys` crates probe their
native libraries with `exactly_version(CARGO_PKG_VERSION)`. A workspace that
mixes v0.0.79 bindings with a v0.0.80 Meson tree fails to link; a local patch
that still pointed at the v0.0.79 checkout would hide that mismatch from the
developer loop.

## Invariants & Behavioral Boundaries

- **`[INV-WKS-06]` still has one dependency to enforce.** The workspace
  resolves Optics Rust code from the tagged `optics` facade only; the settings
  feature change does not introduce a granular dependency.
- **`[INV-PANEL-13]` is now stated at the factory.** The Panel passes
  `FLUX_CANVAS_BACKEND_CPU`; no Vulkan device, surface, swapchain, dma-buf,
  GPU readback, or GPU glyph texture enters the path.
- **`[INV-PANEL-14]` is unchanged.** Every Panel pass still clears its whole
  retained framebuffer to transparent before drawing geometry.
- **The settings application remains pure Lens chrome.** Aperture supplies the
  window and frame loop; the binary never installs a record callback.
- **Native-library discovery stays env-driven.** A `*_BUILD_DIR` variable that
  points at a Meson tree yields an rpath for the corresponding `libs/`
  directory; no manually maintained `LD_LIBRARY_PATH` is required.

## Rejected Alternatives & Negative Knowledge

- **Keep `typio-settings` on the `platform` feature and import from
  `optics::platform`.** Rejected: Iris no longer depends on Lens and no longer
  exports `Application`, so the crate does not compile and the import would
  silently couple settings to the wrong layer.
- **Depend on `optics::ui` and `optics::platform` separately.** Rejected:
  neither provides the application shell; re-adding `aperture` by hand is just
  the `app` feature with more ways to drift.
- **Keep `flux_canvas_create_cpu` behind a compatibility wrapper.** Rejected:
  the symbol is gone from the C library and the bindings. A private wrapper
  would only rename the descriptor, hiding that Optics now has one factory.
- **Rely on `FLUX_CANVAS_BACKEND_AUTO` because the Panel has no surface.**
  Rejected: it makes the no-GPU invariant an inference about a fallback rather
  than an explicit request.
- **Keep `glyph_draw_outlined` or reproduce its two-call shape.** Rejected:
  the symbol is removed; `glyph_draw_opts` already carries the outline colour
  and width in the single draw pass.
- **Adopt the GPU tiled rasterizer or dynamic image descriptors.** Rejected:
  the Panel presents host memory through `wl_shm`, and the tray badge is a
  one-shot CPU readback; neither has a GPU destination.
- **Adopt Reticle, Transit, Prism, or Vista.** Rejected: Typio supplies no
  vector document, runs no motion system in the Panel, and has no material or
  3D scene content.
- **Stay on `v0.0.79`.** Rejected: the sibling checkout and its Cargo workspace
  have moved to v0.0.80, and the exact-version probes make a split graph fail
  to build. Staying would require pinning a private copy of the previous
  bindings and native tree.

## Consequences

- **Positive**: the settings application follows the upstream one-way library
  graph and Aperture's lifecycle surface; the Panel keeps a single explicit
  CPU factory; glyph and canvas calls are one descriptor each.
- **Positive**: local and canonical builds both name the Aperture native
  directory, so settings tests and runs keep the no-`LD_LIBRARY_PATH`
  guarantee.
- **Negative & trade-offs**: the `app` feature is coarser than `platform`; a
  future settings surface that does not need the UI stack would have to split
  the feature again upstream rather than locally.
- **Operational**: `main` must regenerate the canonical lockfile against the
  tagged Git source and pass `cargo check --locked --workspace`; the
  development worktree lockfile remains path-patched and excluded by the commit
  hook.

## Links

- Optics ADR-0163: repository-root Cargo workspace.
- Optics ADR-0164: umbrella feature detection uses distribution flags.
- Optics ADR-0165: single opts descriptor migration by library.
- Optics ADR-0166: one-way library graph and enforced invariants.
- Amends [ADR-0062](0062-optics-v0.0.79-adoption-boundary.md) for the facade
  feature graph, canvas factory, and glyph descriptor surface; its distribution
  decisions remain binding.
- Amends [ADR-0053](0053-optics-v0.0.62-adoption-boundary.md) only where the
  removed `glyph_draw_outlined` entry point is named; the badge outline
  decision is preserved through `glyph_draw_opts`.
- Living blueprint: [Workspace Topology](../architecture/workspace-topology.md)
  and [Panel Rendering](../architecture/panel-rendering.md).
