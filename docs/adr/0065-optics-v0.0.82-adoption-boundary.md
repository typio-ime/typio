---
id: ADR-0065
title: "Optics v0.0.82 adoption boundary and semantic module naming"
status: accepted
date: 2026-11-23
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0065: Optics v0.0.82 adoption boundary and semantic module naming

- **Status**: Accepted
- **Date**: 2026-11-23
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: None
- **Amends**: [ADR-0064](0064-optics-v0.0.81-adoption-boundary.md)

---

## Context

Optics advanced from `v0.0.81` to `v0.0.82` and replaced its metaphorical
codenames with a semantic, brand-scoped namespace (Optics ADR-0170, Optics
ADR-0172):

- **Every layer is one canonical name across C, Meson, `pkg-config`, and
  Rust.** `flux` → `gfx`, `glyph` → `text`, `reticle` → `svg`, `prism` →
  `material`, `vista` → `scene`, `transit` → `motion`, `iris` → `window`,
  `lens` → `ui`, and `aperture` → `app`. The C libraries are
  `liboptics_<module>`, the headers live under `<optics/…>`, and public C
  symbols use the `optics_<module>_` prefix.
- **The facade exposes semantic modules.** `optics` features are now `gfx`,
  `text`, `svg`, `material`, `scene`, `motion`, `window`, `ui`, and `app`;
  the raw FFI re-exports moved from `canvas::sys` / `typography::sys` to
  `gfx::sys` / `text::sys`.
- **The zero-stutter API removes redundant prefixes.** The renamed types and
  entry points Typio touches are now `optics_text_ctx`, `optics_text_desc`,
  `optics_text_draw_opts`, `optics_text_measure_opts`, `optics_text_family`,
  `optics_gfx_canvas_*`, `optics_gfx_color_*`, and
  `OPTICS_GFX_CANVAS_BACKEND_CPU`.
- **Native discovery names follow the layer.** Meson options are `-Dtext`,
  `-Dsvg`, `-Dscene`, `-Dmaterial`, and `-Dmotion` (there is no `-Dglyph`),
  environment overrides use `OPTICS_<MODULE>_*` spellings, and runtime log
  prefixes are `[optics_gfx …]` / `[optics_text …]`.
- **Retired scaffolding (Optics ADR-0173).** The `vault` tessellation helpers
  and mixed-case type aliases are gone; a C23 compiler with `#embed` is now
  required to build the C libraries.

Typio pinned `v0.0.81` (ADR-0064) and still spoke the codename vocabulary in
four places: the facade features (`typography`), the FFI modules
(`canvas::sys` / `typography::sys`), native discovery (`-Dglyph=true`,
`FLUX_*` / `GLYPH_*` build variables, `libs/{flux,glyph,…}` paths, `libflux` /
`libglyph` stems), and the host's own naming (`FluxPanel`, `HAVE_FLUX`).

## Decision

1. **Move the pin atomically to Optics `v0.0.82`.** Update the workspace
   `optics` facade tag to `v0.0.82` and CI `OPTICS_PINNED_REF` to commit
   `028cca6d78578f5ebb990195998f6fbd148c27b6`, regenerating the canonical
   `Cargo.lock` on promotion to `main`.

2. **Adopt the semantic facade features and FFI modules.** The daemon and
   host platform enable `text` instead of `typography`; `typio-settings`
   keeps `app`, which now pulls in `ui`, `window`, `text`, and `gfx`. Raw FFI
   is reached through `optics::gfx::sys` and `optics::text::sys`. Typio does
   not declare a granular Optics crate dependency.

3. **Translate the native discovery layer.** `crates/optics-rpaths.rs` maps
   the Meson subdirectories `gfx`, `text`, `ui`, `window`, `app`, and `svg`
   to the `liboptics_<module>` stem, and accepts
   `OPTICS_GFX_BUILD_DIR`, `OPTICS_TEXT_BUILD_DIR`, `OPTICS_UI_BUILD_DIR`,
   `OPTICS_WINDOW_BUILD_DIR`, `OPTICS_APP_BUILD_DIR`, and
   `OPTICS_SVG_BUILD_DIR` alongside `OPTICS_BUILD_DIR`. The managed Meson
   setup passes `-Dtext=true`.

4. **Remove codenames from Typio's own vocabulary.** Rename `FluxPanel` to
   `Panel` and the generated `HAVE_FLUX` constant to `HAVE_OPTICS_GFX`, and
   rewrite current architecture, how-to, and contributor documentation to the
   semantic module names. Historical ADRs keep their original wording because
   an ADR records the vocabulary in force when it was ratified.

5. **Keep the declined layers declined.** `svg`, `material`, `scene`, and
   `motion` remain absent from Typio's feature set: the Panel is a CPU
   `wl_shm` renderer (`[INV-PANEL-13]`) and `typio-settings` uses flow layout
   with no scene, material, or vector-document path.

6. **Keep the Optics linking contract aligned with ADR-0064.** `v0.0.82` still
   prefers static archives, so `OPTICS_STATIC=1`, conditional rpaths, and the
   release `-O0` gfx guard carry over unchanged here. Optics `v0.0.83` later
   changes the default to shared-preferred and adds a `static` feature; that
   adoption is recorded separately in
   [ADR-0066](0066-optics-v0.0.83-adoption-boundary.md).

## Rationale

**Why rename Typio's own identifiers instead of only the dependency.**
Leaving `FluxPanel` and `HAVE_FLUX` in place would preserve exactly the
translation tax Optics set out to remove: the project would still name its
central type after a retired library. The rename is mechanical, compiler-checked,
and confined to the host platform crate.

**Why update historical ADRs' live links but not their prose.** ADR-0063 and
ADR-0064 described `flux_canvas_create` and `-Dglyph=true` because those were
the real interfaces at the time. Rewriting them would falsify the record and
break the negative-knowledge trail; the new alignment lives here and in the
living blueprints.

**Why the facade still mediates every layer.** Keeping one `optics`
dependency with feature-selected layers preserves `[INV-WKS-06]` and keeps the
local path-patched graph identical in shape to the canonical one
(`.cargo/optics-local.toml` patches only the facade).

## Invariants & Behavioral Boundaries

- **`[INV-WKS-06]` is unchanged.** Workspace crates resolve Optics Rust
  bindings only through the single `optics` facade crate; the semantic feature
  names change the feature graph, not the one-dependency boundary.
- **`[INV-PANEL-13]` and `[INV-PANEL-14]` are unchanged.** The Panel still
  creates only `OPTICS_GFX_CANVAS_BACKEND_CPU` canvases, holds no GPU object,
  and clears its retained framebuffer every pass. Renamed symbols only.
- **No compatibility shims.** Typio references only `optics_*` C symbols and
  semantic facade paths; a stale `v0.0.81` Meson tree fails to link rather
  than silently mixing descriptor layouts.

## Rejected Alternatives & Negative Knowledge

- **Keep re-exporting or aliasing the old codenames.** Rejected: Optics
  ADR-0170 makes a clean break with no compatibility shims, and a local alias
  layer would add a second vocabulary that no other Optics consumer shares.
- **Keep the `typography` facade feature while renaming C symbols.**
  Rejected: the feature ceased to exist upstream, so the manifest would fail
  to resolve; a local feature alias would also break the one-graph shape.
- **Adopt `svg` for the tray badges.** Rejected: badges are rasterised text
  with a coverage outline (`optics_text_draw_opts`), not vector documents;
  adding the layer would link a library no Typio code path reaches.
- **Rename the Panel type to `OpticsPanel`.** Rejected: `Panel` is the
  project's own domain term (ADR-0014); naming it after the renderer would
  re-couple a policy type to a dependency.
- **Adopt the shared-by-default link change earlier than a tagged release.**
  Rejected here: that behaviour lived only in Optics `[Unreleased]` at the
  `v0.0.82` tag. It is adopted on its released terms in
  [ADR-0066](0066-optics-v0.0.83-adoption-boundary.md).

## Consequences

- **Positive**: Typio, its build scripts, and its documentation share one
  vocabulary with Optics, so `gfx`, `text`, `ui`, `window`, and `app` need no
  mental translation.
- **Positive**: Native discovery keeps working against both the pre-`v0.0.82`
  per-library variables and the new `OPTICS_<MODULE>_BUILD_DIR` spellings,
  because `OPTICS_BUILD_DIR` remains the workspace-wide default.
- **Negative & trade-offs**: Existing contributor Meson trees must be
  reconfigured (or rebuilt from scratch) because `-Dglyph` no longer exists;
  an old build tree keeps stale `libflux`/`libglyph` archives that the new
  probe ignores.
- **Operational**: Promoting to `main` regenerates the canonical `Cargo.lock`
  against the tagged Optics release in effect (`v0.0.83`; see
  [ADR-0066](0066-optics-v0.0.83-adoption-boundary.md)) and verifies
  `cargo check --locked --workspace`.

## Links

- Optics ADR-0170: semantic module naming and unified `optics` namespace.
- Optics ADR-0172: zero-stutter API and the `window`/`app` boundary.
- Optics ADR-0173: retired vault and compatibility scaffolding.
- Amends [ADR-0064](0064-optics-v0.0.81-adoption-boundary.md) for the
  `v0.0.82` pin, semantic module names, and facade feature renames.
- Living blueprints: [Workspace Topology](../architecture/workspace-topology.md)
  and [Panel Rendering](../architecture/panel-rendering.md).
