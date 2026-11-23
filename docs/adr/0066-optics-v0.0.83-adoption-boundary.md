---
id: ADR-0066
title: "Optics v0.0.83 adoption boundary and explicit static linking"
status: accepted
date: 2026-11-23
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0066: Optics v0.0.83 adoption boundary and explicit static linking

- **Status**: Accepted
- **Date**: 2026-11-23
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: None
- **Amends**: [ADR-0064](0064-optics-v0.0.81-adoption-boundary.md),
  [ADR-0065](0065-optics-v0.0.82-adoption-boundary.md)

---

## Context

Optics advanced from `v0.0.82` to `v0.0.83` and inverted its Rust linking
default:

- **Shared-preferred by default.** With no feature and no environment
  variable, every `optics-*-sys` crate now links `liboptics_*.so` when it
  exists and falls back to the archives only on a static-only tree.
- **A new additive `static` feature.** Every `-sys` crate, wrapper crate, and
  the `optics` facade exposes it, forwarded down the graph:
  `optics = { ..., features = ["static"] }`. It fails when an archive or a
  private dependency is missing instead of silently degrading to a shared
  link. There is deliberately no `dynamic` feature because Cargo features are
  additive and unified across the graph.
- **Environment overrides keep top priority.** `OPTICS_STATIC` and
  `OPTICS_DYNAMIC` can force either mode over the `static` feature;
  `PKG_CONFIG_ALL_STATIC` / `PKG_CONFIG_ALL_DYNAMIC` are honoured as aliases.
- **One link mode per build.** Each `-sys` crate publishes
  `cargo:link_mode=static|dynamic`, dependents adopt the upstream choice, and
  a contradiction is a hard error. A static link checks the whole
  `pkg-config --static` closure up front.
- **Retired scaffolding (Optics ADR-0173).** The `vault` tessellation helpers
  are gone and building the C libraries now requires a C23 compiler with
  `#embed` (GCC >= 15, Clang >= 19).

ADR-0064 established that Typio release binaries bundle Optics statically:
no Optics `DT_NEEDED` entries and no build-tree rpaths. ADR-0065 kept that
contract by relying on `OPTICS_STATIC=1` in `.cargo/optics-local.toml`, CI, and
contributor documentation. That environment variable masks the upstream
default change in local and CI builds, but a bare canonical build
(`cargo build --release`, `cargo install`, a packager's clean environment)
would now silently link shared Optics libraries.

## Decision

1. **Move the pin atomically to Optics `v0.0.83`.** Update the workspace
   `optics` facade tag to `v0.0.83`, add the facade `static` feature to that
   dependency, and update CI `OPTICS_PINNED_REF` to commit
   `646952c558fd612acb1935d11fdb12c62d2323de`, regenerating the canonical
   `Cargo.lock` on promotion to `main`.

2. **Request static linking in the manifest, not only in the environment.**
   The workspace dependency becomes
   `optics = { git = "…", tag = "v0.0.83", default-features = false, features = ["static"] }`.
   Every consumer's `optics = { workspace = true, features = [...] }` inherits
   it, so a clean canonical build keeps the self-contained binaries that
   ADR-0064 promised. `OPTICS_STATIC=1` in `.cargo/optics-local.toml`,
   `.cargo/config.example.toml`, and CI stays as an explicit, equivalent
   override (and as documentation of the intent).

3. **Keep the dynamic escape hatch unchanged.** `OPTICS_DYNAMIC=1` overrides
   the feature and selects shared linking; the Typio build scripts continue to
   bake build-tree `-Wl,-rpath` entries only in that mode, and the release
   `-O0` gfx guard still runs regardless of the link mode.

4. **Keep the C build tree at `default_library=both`.** The `static` feature
   fails loudly when an archive is missing, so developer and CI Meson trees
   must produce archives alongside the shared objects (Optics's upstream
   default). A static-only tree is also acceptable.

5. **Do not adopt Optics's guardrail tooling wholesale.** The new
   `check_leaks`, `check_listeners`, and naming guardrails police the Optics
   tree. Typio keeps its own valgrind leak gate and documentation checks; no
   Optics tool becomes a Typio build dependency.

## Rationale

**Why the feature instead of the environment variable.** `OPTICS_STATIC=1`
works only where a shell or CI job sets it. A packager, a distro build
service, or `cargo install` from a clean environment would get the new
shared-preferred default and ship binaries with an undeclared runtime
dependency on `liboptics_*.so`. Declaring `static` in the manifest makes the
linking contract part of the dependency graph, which is the only place a
canonical build reads.

**Why the facade feature rather than per-crate features.** `[INV-WKS-06]`
keeps every workspace crate behind the single `optics` facade. Enabling
`static` once on the facade forwards to exactly the layers each consumer
enables, and Cargo's feature unification guarantees one link mode per build —
the property Optics now enforces mechanically.

**Why not drop the environment overrides.** They are the documented,
top-priority escape hatch for packaging shared binaries
(`OPTICS_DYNAMIC=1`), and keeping them costs nothing. The manifest feature
states intent; the environment states a deliberate exception.

## Invariants & Behavioral Boundaries

- **`[INV-WKS-06]` is unchanged.** One `optics` facade dependency mediates
  every Optics layer; the `static` feature changes only how the native
  libraries attach.
- **Static release binaries carry no Optics `DT_NEEDED` or rpath.** When
  `OPTICS_DYNAMIC` is unset, `typio` and `typio-settings` embed the archives
  and keep system libraries dynamic.
- **`[INV-PANEL-13]` and `[INV-PANEL-14]` are unchanged.** Linking mode does
  not introduce a GPU object or change the Panel pass.

## Rejected Alternatives & Negative Knowledge

- **Rely on `OPTICS_STATIC=1` alone.** Rejected: it is invisible to canonical
  `cargo build`/`cargo install` invocations and would let the shared default
  leak into release packaging unnoticed.
- **Enable `static` per consumer crate.** Rejected: features unify anyway, and
  repeating it in three manifests invites drift; the workspace dependency is
  the single declaration point.
- **Adopt the new shared default and ship `liboptics_*.so`.** Rejected: it
  changes the deployment contract (runtime library path, package splitting,
  SONAME tracking) for no benefit to a daemon that favours a self-contained
  binary.
- **Use `-Ddefault_library=static` to guarantee archives.** Rejected: it
  removes the shared objects that `OPTICS_DYNAMIC=1` and the rpath fallback
  need; `both` is the upstream default and serves both modes.
- **Vendor Optics's leak/listener guardrails into Typio CI.** Rejected: they
  encode Optics-internal lane policy; Typio's existing valgrind gate covers
  the FFI paths it actually links.

## Consequences

- **Positive**: A bare `cargo build --release` on `main` produces the same
  self-contained Optics linkage as the documented developer recipe.
- **Positive**: The manifest now states the linking contract that ADR-0064
  described, and the environment variable becomes an explicit override rather
  than invisible configuration.
- **Negative & trade-offs**: The workspace now requires Optics archives at
  build time; a shared-only Meson tree fails unless `OPTICS_DYNAMIC=1` is set.
  The failure is deliberate and names the missing module.
- **Operational**: Promoting to `main` regenerates the canonical `Cargo.lock`
  against tag `v0.0.83` and verifies `cargo check --locked --workspace`.

## Links

- Optics ADR-0173: retired vault and compatibility scaffolding.
- Amends [ADR-0064](0064-optics-v0.0.81-adoption-boundary.md) for the
  `v0.0.83` pin and explicit `static` feature selection.
- Continues [ADR-0065](0065-optics-v0.0.82-adoption-boundary.md) for the
  semantic module names.
- Living blueprints: [Workspace Topology](../architecture/workspace-topology.md)
  and [Panel Rendering](../architecture/panel-rendering.md).
