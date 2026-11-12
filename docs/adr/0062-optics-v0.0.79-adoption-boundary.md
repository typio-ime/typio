---
id: ADR-0062
title: "Optics v0.0.79 adoption boundary"
status: accepted
date: 2026-11-12
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0062: Optics v0.0.79 adoption boundary

- **Status**: Accepted
- **Date**: 2026-11-12
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: None

---

## Context

Optics advanced from `v0.0.78` to `v0.0.79`. This interval is a
distribution-surface release rather than a rendering redesign:

- **Unified Rust bindings workspace and facade crate (Optics ADR-0159).** The
  eight per-module Cargo workspaces (`bindings/flux-rs`, `bindings/lens-rs`,
  and siblings) collapsed into one workspace under `bindings/`. External
  consumers are now expected to depend on the single `optics` facade crate,
  whose feature flags re-export each layer (`optics::canvas`,
  `optics::typography`, `optics::ui`, `optics::platform`, …). The old crate
  paths no longer exist.
- **Unified C facade and umbrella metadata (Optics ADR-0160).**
  `<optics/optics.h>` and `optics.pc` join the existing granular headers and
  `pkg-config` packages. The shared objects remain separate; the umbrella is
  purely distributive.
- **Reticle and Vista boundary (Optics ADR-0161).** The SVG vector-document
  compiler was renamed to `reticle` (`libreticle.so`, `<reticle/reticle.h>`,
  `reticle_*` symbols), Lens icon loading now routes through that parser, and
  the Vista/GLB suites moved out of the Flux test tree.
- **Transit header-only motion vocabulary (Optics ADR-0158).** Transit lost its
  shared object and FFI crate; it is now a header-only C vocabulary and a pure
  Safe Rust crate.

The public C headers Typio actually consumes — `flux`, `glyph`, `lens`, and
`iris` — change only their version-patch macros. The load-bearing change is the
Rust distribution surface: Typio currently declares five direct Optics
dependencies (`flux-sys`, `glyph-sys`, `iris`, `iris-sys`, `lens-sys`), and its
linked development worktree patches seven crate paths that no longer exist.

Typio has two Optics consumers with different layer needs:

- The **daemon and host platform** render the candidate Panel through the CPU
  Flux canvas and shape text through Glyph. They use the raw `-sys` escape
  hatches directly; they do not need Lens or Iris.
- **`typio-settings`** is an Iris application built from Lens chrome. It needs
  the platform, UI, typography, and canvas layers, but no materials, scene,
  reticle, or motion layer.

## Decision

1. **Move the pin atomically to Optics `v0.0.79`.** Update the CI
   `OPTICS_PINNED_REF` to commit
   `63a92ba1aad53dfcef42c9d6e31a96b9d9de4a46` (tag `v0.0.79`) and regenerate
   the canonical `Cargo.lock` when the change is promoted to `main`.

2. **Adopt the `optics` facade as the single Optics Rust dependency.**
   `[workspace.dependencies]` declares one `optics` entry with
   `default-features = false`. Consumers select only their layers:
   - `typio-daemon` and `typio-host-platform`: `features = ["typography"]`
     (which implies `canvas`);
   - `typio-settings`: `features = ["platform"]` (which implies `ui`,
     `typography`, and `canvas`).

   Raw bindings are reached through the facade's documented escape hatches,
   `optics::canvas::sys` and `optics::typography::sys`, rather than by adding
   granular `-sys` crates back to the workspace.

3. **Re-derive native-library rpaths in Typio's build scripts.** The facade
   hides the `-sys` crates, so their `links` metadata
   (`DEP_FLUX_RPATHS`/`DEP_GLYPH_RPATHS`) is no longer visible to Typio's build
   scripts. Each affected build script now discovers the Meson tree from the
   same `FLUX_BUILD_DIR`, `GLYPH_BUILD_DIR`, `LENS_BUILD_DIR`, and
   `IRIS_BUILD_DIR` variables the bindings consume, and emits rpaths for the
   existing `libs/{flux,glyph,lens,iris,reticle}` directories.

4. **Patch only the facade in local development mode.**
   `.cargo/optics-local.toml` redirects
   `../optics/bindings/crates/optics`; its path dependencies resolve the rest
   of the unified bindings workspace from the sibling checkout. This keeps the
   local graph the same shape as the canonical one.

5. **Adopt the Lens icon-parser migration transitively.** Lens now parses icon
   documents through Reticle instead of its retired duplicate scanner. Typio
   supplies no SVG icon documents to Lens and calls no Reticle symbol directly,
   so the change reaches the settings application only as an upstream
   implementation detail.

6. **Decline the C umbrella facade for this repository.** Typio has no C
   consumer to include `<optics/optics.h>` or link `optics.pc`; the `-sys`
   build scripts continue to probe the granular libraries they wrap.

7. **Decline direct Reticle, Transit/motion, Prism, and Vista dependencies.**
   The Panel draws geometry and glyph runs, tray badges are glyph runs, and the
   settings application is Lens chrome. None of these layers has a reachable
   caller here.

## Rationale

**Why the facade is the right adoption.** Optics now treats the facade as the
canonical external Rust surface (Optics ADR-0159), and the old per-module
workspaces are gone. Keeping five direct dependencies at `v0.0.79` would be a
private fork of the old distribution shape: it would bypass the facade's
feature graph, reintroduce the diamond-resolution problem the upstream
consolidation removed, and require five lockfile updates per release instead
of one. Depending on the facade also makes feature intent reviewable: a crate
manifest states which engine layers a binary is allowed to reach.

**Why the feature sets are narrow.** `typography` is the smallest feature that
provides both the Flux canvas and the Glyph shaper used by the Panel and badge
paths. `platform` is the smallest feature that provides Iris plus its Lens
dependency for the settings application. Enabling default features everywhere
would pull `ui`, `platform`, and `motion` into the daemon, linking windowing and
retained-UI libraries the daemon never calls. The raw `-sys` re-exports keep the
existing unsafe calls honest without adding a second dependency route around
the feature graph.

**Why the rpath derivation moved into Typio.** Cargo passes a dependency build
script's `rustc-link-arg` only to that dependency's own artifacts, and its
`DEP_<links>_*` metadata only to direct dependents. The old graph made
`flux-sys` and `glyph-sys` direct dependencies of the daemon and host platform,
which let their build scripts read and re-emit the rpaths. Reaching the same
libraries through `optics` removes that relationship. Deriving the paths from
the documented build-tree variables is the smallest mechanism that preserves
the local "no `LD_LIBRARY_PATH`" guarantee without adding the granular crates
back or changing Optics.

**Why no host behaviour changes.** The consumed C entry points and descriptor
layouts are unchanged; only the version-patch macros moved. The v0.0.79 release
notes change how Lens obtains icon geometry and where Optics keeps its
scene-graph tests, neither of which is on a Typio path. The CPU-canvas
`wl_shm` Panel, full-frame repaint, non-blocking buffer back-pressure, and the
record-only Iris contract stay exactly as ADR-0061 left them.

**Why the pin and local paths move together.** The `-sys` build scripts require
the exact crate version at `pkg-config` time, so a partially advanced graph
fails to link rather than silently reading an older descriptor layout. Local
development mode must also move because the old `bindings/*-rs` paths no longer
exist in the sibling checkout; a stale `.cargo/config.toml` would fall back to a
system Optics install.

## Invariants & Behavioral Boundaries

- **`[INV-WKS-06]` now has one dependency to enforce.** The workspace resolves
  Optics Rust code from the tagged `optics` facade only; no crate declares a
  direct `flux`, `glyph`, `lens`, `iris`, or `-sys` Optics dependency.
- **`[INV-WKS-07]` keeps local mode contained.** The local patch redirects the
  facade path, and neither `.cargo/config.toml` nor the path-resolved
  `Cargo.lock` is committed.
- **`[INV-PANEL-13]` and `[INV-PANEL-14]` are unaffected.** No GPU object enters
  the Panel, and every Panel pass still clears its whole framebuffer.
- **The settings application remains pure Lens chrome.** It registers no host
  canvas callback; `optics::platform` replaces the former direct `iris` import
  without changing the call shape.
- **Native-library discovery stays env-driven.** When a `*_BUILD_DIR` variable
  points at a Meson tree, the consuming Typio binary carries an rpath for the
  existing `libs/` directories. No shell needs a manually maintained
  `LD_LIBRARY_PATH` in the development loop.

## Rejected Alternatives & Negative Knowledge

- **Keep the five granular dependencies at `v0.0.79`.** Rejected: the crate
  paths exist in the new workspace, but this preserves the retired distribution
  shape, bypasses the facade feature graph, and keeps five-way version drift
  possible.
- **Depend on `optics` with default features in every crate.** Rejected: the
  daemon and host platform would link Lens, Iris, and the motion crate for
  symbols they never call, undoing the upstream pay-for-what-you-use design.
- **Keep direct `-sys` dependencies alongside the facade.** Rejected: it adds a
  second route around the feature graph and lets a future editor bump one
  family independently. The facade already re-exposes the raw bindings.
- **Adopt `<optics/optics.h>` or `optics.pc`.** Rejected: Typio ships no C
  translation unit and its Rust `-sys` crates bindgen granular headers; an
  umbrella dependency would add linkage without a caller.
- **Adopt Reticle for tray or panel art.** Rejected: tray icons are themed PNGs
  delivered through SNI and language badges are glyph runs. There is no vector
  document to compile.
- **Adopt Transit/motion for the Panel or settings.** Rejected: the Panel is
  event-driven with no animation, and settings delegates its caret and frame
  pacing to Iris/Lens.
- **Adopt Prism materials or Vista scene support.** Rejected: the Panel is a
  CPU canvas with solid fills, and the settings application has no 3D content.
- **Stay on `v0.0.78`.** Rejected: the release is the current tag, the local
  patch paths have already moved, and the upstream facade is the supported
  consumer surface. Staying would require maintaining a private multi-crate
  dependency block against a workspace layout that no longer exists.

## Consequences

- **Positive**: one dependency line, one local patch path, one lockfile entry
  family, and feature flags that state each binary's layer budget.
- **Positive**: `scripts/optics-release-ref.sh` now validates one tagged Optics
  dependency instead of five.
- **Negative & trade-offs**: the `-sys` escape hatches are transitive
  re-exports, so a future raw API needs the facade to expose it (or a deliberate
  workspace-level exception). This is the cost of a single supported surface.
- **Operational**: `main` must regenerate the canonical lockfile against the
  tagged Git source and pass `cargo check --locked --workspace`; the development
  worktree lockfile remains path-patched and excluded by the commit hook.

## Links

- Optics ADR-0158: Transit is a header-only motion vocabulary.
- Optics ADR-0159: unified Cargo workspace and `optics` facade bindings.
- Optics ADR-0160: unified C facade and umbrella distribution.
- Optics ADR-0161: Reticle vector compiler and Vista boundary.
- Amends [ADR-0061](0061-optics-v0.0.78-adoption-boundary.md) for the
  distribution surface only; its rendering and Panel decisions remain binding.
- Living blueprint: [Workspace Topology](../architecture/workspace-topology.md).
