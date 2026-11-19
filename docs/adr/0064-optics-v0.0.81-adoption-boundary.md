---
id: ADR-0064
title: "Optics v0.0.81 adoption boundary and static-first linking"
status: accepted
date: 2026-11-20
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0064: Optics v0.0.81 adoption boundary and static-first linking

- **Status**: Accepted
- **Date**: 2026-11-20
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: None

---

## Context

Optics advanced from `v0.0.80` to `v0.0.81` and updated its native build and
binding linkage contract:

- **Exported dma-buf image leases (Optics ADR-0168).**
  `flux_surface_export_dmabuf` and `flux_surface_export_dmabuf_explicit` now
  return a `flux_surface_dmabuf_lease` (Rust `ExportedDmabuf { dmabuf, lease }`),
  `flux_result` gains `FLUX_ERROR_BUSY`, and `_pooled` variants cover
  persistent slot tables.
- **Scoped frame clip in Lens (Optics ADR-0167).** `lens_clip_begin` /
  `lens_clip_end` (`Frame::clip_begin` / `Frame::clip_end`) stamp a rectangular
  frame-scoped clip that follows absolutely placed overlays into higher z-bands.
- **Root `crates/` layout and `optics-*` namespace (Optics ADR-0169).** Optics
  moved its Rust workspace crates from `bindings/crates/*` to repository-root
  `crates/*` and prefixed internal crates with `optics-*` while keeping the
  `optics` facade API unchanged. Local worktree patches therefore target
  `../optics/crates/optics`.
- **Static-first Optics linking and unified environment overrides.** Optics
  switched its Meson default to `default_library=both` (shipping static
  archives alongside shared libraries), and all `*-sys` build scripts now
  prefer static Optics archives (`lib*.a` / `*.lib` via `static:-bundle=<name>`)
  while keeping system dependencies dynamic. They accept `OPTICS_BUILD_DIR` and
  `OPTICS_SOURCE_DIR` as single workspace-wide overrides alongside
  `OPTICS_STATIC=1` and `OPTICS_DYNAMIC=1`, and emit empty `cargo:rpaths=` when
  linking statically.

Typio's existing build scripts (`crates/optics-rpaths.rs`,
`crates/typio-host-platform/build.rs`, `crates/typio-settings/build.rs`) and
Cargo templates assumed shared Optics libraries and ten per-library environment
variables:

- `crates/optics-rpaths.rs` unconditionally emitted `-Wl,-rpath` for every
  `libs/<name>` directory it found, baking private build-tree `DT_RUNPATH` /
  `DT_RPATH` tags into statically linked binaries that had zero Optics
  `DT_NEEDED` entries.
- `crates/typio-host-platform/build.rs` derived Meson build-option discovery
  from `optics_native_rpaths()`. Making rpaths empty for static builds would
  have silently bypassed the release `-O0` Flux guard unless build-directory
  discovery was decoupled from rpath emission.
- `crates/typio-settings/build.rs` unconditionally emitted
  `-Wl,--disable-new-dtags` even when no rpaths were needed.

## Decision

1. **Move the pin atomically to Optics `v0.0.81`.** Update the workspace
   `optics` facade tag to `v0.0.81` and CI `OPTICS_PINNED_REF` to commit
   `35766d5cffb332fdc41d7bf52fc04bfb2d2158f5`, regenerating the canonical
   `Cargo.lock` on promotion to `main`.

2. **Adopt Optics's static bundling (`OPTICS_STATIC=1`) and unified overrides.**
   Switch CI, `.cargo/optics-local.toml`, `.cargo/config.example.toml`, and
   contributor documentation to `OPTICS_BUILD_DIR`, `OPTICS_SOURCE_DIR`, and
   `OPTICS_STATIC=1` (keeping per-library `*_BUILD_DIR` variables honored for
   backward compatibility and `OPTICS_DYNAMIC=1` for explicit shared builds).

3. **Emit rpaths only for dynamically linked Optics libraries.** Update
   `crates/optics-rpaths.rs` to mirror Optics's `*-sys` linkage rule: when a
   static archive (`lib*.a` / `*.lib`) is present and `OPTICS_DYNAMIC` is unset,
   emit no rpath; emit `-Wl,-rpath` (and `-Wl,--disable-new-dtags` in
   `typio-settings`) only when Optics is linked dynamically.

4. **Decouple the release `-O0` guard from rpath emission.** Add
   `optics_build_dirs()` in `crates/optics-rpaths.rs` and pass it to
   `enforce_optimized_flux_for_release` in `crates/typio-host-platform/build.rs`
   so Cargo release builds continue rejecting unoptimized `-O0` Meson Flux trees
   whether Optics is linked statically or dynamically.

5. **Decline Flux dma-buf leases and Lens scoped frame clips.** Neither
   upstream addition has a precondition in Typio: the Panel renders strictly on
   the CPU into `wl_shm` (`[INV-PANEL-13]`), and `typio-settings` uses flow
   layout with no absolutely placed overlay strips.

## Rationale

**Why conditional rpaths instead of deleting `crates/optics-rpaths.rs`.**
Optics still supports dynamic linking (via `OPTICS_DYNAMIC=1`, shared-only
Meson trees such as `v0.0.81`'s default CI build, or custom prefixes). Mirroring
the exact static-vs-dynamic predicate used by the `-sys` crates gives static
binaries zero build-tree rpaths while preserving zero-`LD_LIBRARY_PATH`
execution for shared builds.

**Why unified `OPTICS_BUILD_DIR` / `OPTICS_SOURCE_DIR`.** Every Optics `-sys`
crate now checks `OPTICS_BUILD_DIR` and `OPTICS_SOURCE_DIR` as the workspace
default. Replacing ten per-library environment variables with two eliminates
drift whenever Optics adds or reorganizes a library layer.

**Why build-directory inspection is separate from rpaths.** The `-O0` Flux
guard protects Panel latency on CPU rendering regardless of archive format; a
static `-O0` `libflux.a` is just as slow as a shared `-O0` `libflux.so`.

## Invariants & Behavioral Boundaries

- **`[INV-WKS-06]` is unchanged.** The workspace resolves Optics Rust bindings
  only through the single `optics` facade crate; static linking changes how
  native archives attach to Typio binaries without vendoring Optics source.
- **`[INV-PANEL-13]` is unchanged.** No Vulkan surface or dma-buf lease enters
  the Panel path.
- **Static binaries carry no Optics rpath or `DT_NEEDED`.** When built against
  a static Optics tree (`OPTICS_DYNAMIC` unset), `typio` and `typio-settings`
  record neither `libflux.so`/`libglyph.so`/`liblens.so`/`libiris.so`/`libaperture.so`
  nor build-tree `DT_RPATH`/`DT_RUNPATH` entries; system libraries remain
  dynamically linked.

## Rejected Alternatives & Negative Knowledge

- **Keep emitting build-tree rpaths unconditionally.** Rejected: static
  binaries would leak developer-local Meson directories into `DT_RUNPATH` /
  `DT_RPATH` despite having no shared Optics dependencies.
- **Remove `crates/optics-rpaths.rs` entirely.** Rejected: breaks `OPTICS_DYNAMIC=1`
  and shared-only Optics trees (including canonical `v0.0.81` CI before static
  linking is tagged upstream).
- **Skip `enforce_optimized_flux_for_release` when `rpaths` is empty.**
  Rejected: would silently allow `cargo build --release` to embed an `-O0`
  `libflux.a`, regressing Panel frame times.
- **Adopt Flux dma-buf leases or Lens scoped frame clips.** Rejected: Typio has
  no Vulkan offscreen surface or dma-buf export in the Panel (`[INV-PANEL-13]`)
  and no absolutely placed overlay chrome in `typio-settings`.

## Consequences

- **Positive**: Statically linked `typio` and `typio-settings` binaries are
  self-contained with respect to Optics (only OS/system libraries remain
  dynamic) and carry no build-tree rpaths.
- **Positive**: Developer and CI configuration drops from ten per-library
  environment variables to `OPTICS_BUILD_DIR` and `OPTICS_SOURCE_DIR`.
- **Negative & trade-offs**: `crates/optics-rpaths.rs` mirrors the `-sys`
  static-vs-dynamic detection heuristic; if Optics changes its archive
  selection rule, the helper must move with it.
- **Operational**: Promoting to `main` regenerates the canonical `Cargo.lock`
  against tag `v0.0.81` and verifies `cargo check --locked --workspace`.

## Links

- Optics ADR-0167: scoped frame clip follows placed overlays.
- Optics ADR-0168: exported dma-buf images carry a lease.
- Amends [ADR-0063](0063-optics-v0.0.80-adoption-boundary.md) for the `v0.0.81`
  pin, unified `OPTICS_BUILD_DIR`/`OPTICS_SOURCE_DIR` overrides, and
  static-first native linking.
- Living blueprint: [Workspace Topology](../architecture/workspace-topology.md)
  and [Panel Rendering](../architecture/panel-rendering.md).
