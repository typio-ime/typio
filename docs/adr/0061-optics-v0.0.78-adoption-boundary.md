---
id: ADR-0061
title: "Optics v0.0.78 adoption boundary"
status: accepted
date: 2026-11-11
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0061: Optics v0.0.78 adoption boundary

- **Status**: Accepted
- **Date**: 2026-11-11
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: None

---

## Context

Optics advanced from `v0.0.66` to `v0.0.78`, twelve patch releases that are not
a routine dependency bump. The interval contains a redesign of frame damage
(Optics ADR-0143 through ADR-0155), a change to the Iris host-canvas contract
(Optics ADR-0127 and the v0.0.78 removal of `iris_paint_fn`), the SVG compiler
(Optics ADR-0150), deferred effects and analytic SDFs (ADR-0147, ADR-0148), the
Slang shader migration (ADR-0156), DisplayList resource ownership and
serialization (ADR-0157), and the first RenderPlan slice (ADR-0089, ADR-0092).

As with ADR-0053 and ADR-0058, the question is not only which tag to pin but
which upstream designs this host should adopt. Typio has two UI surfaces with
opposite constraints:

- The **candidate Panel** is a CPU-only Flux canvas presented through `wl_shm`
  on the Wayland reactor thread. It owns no `iris` application, no Vulkan
  device, and no swapchain; it repaints only when the composition sequence or
  presentation gate changes.
- **`typio-settings`** is a conventional `iris` application using Lens chrome.
  It has no host canvas content, so it has never supplied a paint or record
  callback.

The interval also contains required source migrations. Iris deleted
`iris_paint_fn`, `iris_app_opts.paint`, `PaintHost`, and
`iris_wayland_create_activation_token`; Flux replaced the pass descriptor's
`render_offset_*` / `render_width/height` fields with a damage set and
`clear_extent`; Lens deleted `lens_damage_rect` in favour of
`lens_damage_rects`; Glyph deleted the single-page atlas compatibility view; and
Prism removed the `frost` aliases. `typio-settings` still called the removed
three-argument `Application::run` with `None::<fn(PaintHost)>`, so the pin cannot
move without a code change.

## Decision

1. **Move the pin atomically to Optics `v0.0.78`.** Advance the five workspace
   dependencies (`flux-sys`, `glyph-sys`, `iris`, `iris-sys`, `lens-sys`) and
   `OPTICS_PINNED_REF` in `.github/workflows/ci.yml` together, and regenerate
   the canonical `Cargo.lock` in the primary worktree during promotion, all
   against commit `eaa66e6a85094bca901d5857776c2360cf9cdbf7`.

2. **Adopt the record-only Iris host-canvas contract in `typio-settings`.**
   Use `Application::run(config, build)` with no paint handle. The settings
   application is pure Lens chrome, so it supplies no record callback either;
   it does not reintroduce a second synchronous drawing path, and it does not
   create a host canvas merely to exercise the API.

3. **Declare the Panel's whole-extent pass explicitly.** `begin_panel_pass`
   keeps its transparent clear and now sets `clear_extent: true`, stating that
   the Panel repaints its entire destination rather than relying on the
   empty-damage inference. The present path keeps `wl_surface.damage_buffer`
   (the full buffer-local rectangle), which is the correct coordinate space at
   a buffer scale other than one; the stale "damage (9)" comment is corrected.

4. **Accept the Lens damage and caret designs transitively through Iris.**
   Regional damage (`lens_damage_rects`), paint-bounds localisation, the caret
   blink phase, and the terminating ease are consumed by Iris's frame pacer.
   `typio-settings` adds no repaint or blink model of its own; doing so would
   duplicate the clock that ADR-0151 moved into Lens.

5. **Keep the Panel on full-frame repaint and decline regional damage.** The
   Panel's three-buffer `wl_shm` pool returns the first free buffer, so
   successive frames can land in different buffers with different contents.
   A partial pass would leave the untouched region of the newly acquired buffer
   holding stale pixels. The correctness precondition from ADR-0053 — a
   single-buffer pool or per-buffer validity tracking — still does not exist,
   and the bounded panel extent already keeps a full repaint cheap.

6. **Decline the GPU-only and retained-content designs for this host.** The
   RenderPlan executor, deferred effect passes, analytic SDFs, Slang shaders,
   Prism materials, image snapshots, dynamic image slots, and DisplayList
   serialization have no reachable consumer in a CPU-canvas `wl_shm` host or
   in a pure-chrome settings application. The SVG compiler likewise has no
   input: tray icons are theme PNG assets, and language badges are glyph runs,
   not SVG documents.

## Rationale

**Why the pin is the first decision.** The `-sys` build scripts still floor
`pkg-config` far below this repository's pin, so a stale system Optics install
can satisfy the probe and link silently. The tag, the resolved commit, the
lockfile, and the CI reference are only correct as one unit. A partial bump
would also mix a v0.0.78 bindgen surface with older C descriptors, which the
pre-1.0 SONAME policy no longer tolerates: from v0.0.76 the SONAME carries the
full version, so a mismatched library fails at load time instead of being read
through the wrong layout.

**Why the settings migration is the record-only contract.** The deleted paint
callback existed for hosts that drew into a live canvas synchronously on the UI
thread. Typio Settings has no such content: every pixel it owns is a Lens
widget. Calling `Application::run` without a record closure is therefore the
honest expression of the host's shape, not a temporary port. If the settings
application ever gains a canvas surface, the supported path is a
`RecordHost` closure that records an immutable DisplayList, never a live paint
callback.

**Why the Panel states `clear_extent` explicitly.** ADR-0149 and ADR-0154 make
the distinction between "the whole destination is new" and "the damage set is
empty" load-bearing: an empty set means no pixels are new, while
`clear_extent` authorises a whole-destination clear. The Panel has always
repainted the whole frame, but the old descriptor only expressed that through
the clear colour and an empty damage set. The explicit flag removes the
inference and keeps the invariant legible if Flux later changes how an empty
damage set interacts with a clear. The CPU backend ignores the field today;
the value is the declaration, not a rendering shortcut.

**Why regional damage still does not apply.** Flux's windowed damage design
solves the rotating-buffer problem with a persistent content image plus an
eight-frame write-epoch history (ADR-0149, ADR-0155). Typio's Panel has neither:
it rasterises directly into the acquired `wl_shm` buffer and keeps no retained
content image. Adopting `flux_damage` on the pass without that machinery would
trade a small per-frame saving for stale pixels on screen, exactly the failure
mode ADR-0144 measured on a swapchain. The Panel's bounded extent policy
(ADR-0044) already keeps the full clear-and-draw cheap, so there is no measured
win to justify a new present-path design.

**Why the Lens benefits need no Typio code.** `typio-settings` does not call
`lens_frame_needs_repaint`, `lens_damage_rects`, or the caret clock itself; Iris
does. The host contract is to build chrome and return, which the settings
application already follows. Adding a second damage query or blink timer in
Typio would be a copy of an engine invariant waiting to drift, and it would
reintroduce the idle-repaint defect ADR-0151 removed.

## Invariants & Behavioral Boundaries

- **`[INV-WKS-06]` remains binding.** Canonical builds resolve Optics bindings
  from one tagged Git release; local path overrides stay isolated to a
  development worktree. The five direct dependencies move together.
- **`[INV-PANEL-13]` is reaffirmed.** No GPU object, swapchain, dma-buf, or GPU
  readback enters the Panel path. The v0.0.78 RenderPlan and damage-epoch
  designs do not change the CPU-canvas-to-`wl_shm` present mechanism.
- **`[INV-PANEL-14]` is made explicit.** Every Panel pass clears its whole
  framebuffer to transparent and declares that intent through `clear_extent`;
  a null descriptor (Flux's load path) remains forbidden.
- The settings application owns no canvas callback. A future host canvas must
  use the record path, not a resurrected paint path.

## Rejected Alternatives & Negative Knowledge

- **Stay on `v0.0.66`.** Rejected: `typio-settings` no longer compiles against
  the current Iris bindings because `PaintHost` and the three-argument
  `Application::run` are gone, and holding the pin twelve patches back keeps
  the stale-`pkg-config` and full-version-SONAME hazards live.
- **Keep a live paint callback by forking or shimming Iris.** Rejected: the
  callback has no consumer in this host, upstream deleted it deliberately, and
  a local shim would restore the UI-thread rendering path that ADR-0127
  removed.
- **Give `typio-settings` a record callback anyway.** Rejected: the application
  has no document surface, image, or custom canvas content. A record closure
  that draws nothing would add a second callback and a display-list pass for
  no pixels.
- **Adopt regional damage for the Panel without changing the SHM pool.**
  Rejected: with three rotating buffers, a partial pass advances one buffer
  while the compositor may present another whose stale remainder was never
  repainted. This is the same failure mode ADR-0053 and Optics ADR-0144
  documented.
- **Pin the Panel to one SHM buffer to enable regional damage.** Rejected: it
  would trade the non-blocking present path and its release back-pressure for a
  per-keystroke saving that the bounded extent policy already keeps small.
  Revisit only if a single-buffer or per-buffer-validity design is justified by
  measurement.
- **Adopt the RenderPlan executor, deferred effects, SDFs, Slang, Prism,
  snapshots, dynamic image slots, or DisplayList serialization.** Rejected:
  none has a reachable consumer here. The Panel never creates a `flux_frame` or
  Vulkan device; `typio-settings` does not record host canvas content; and the
  tray badge path is an immediate CPU draw, not a retained display list.
- **Adopt the SVG compiler for tray or badge art.** Rejected: Typio receives
  themed PNG icon files through SNI and renders language badges as glyph runs.
  There is no SVG document to parse.
- **Move only the dependencies that fail to compile.** Rejected: a split pin
  produces a bindgen surface and a linked C library from different releases,
  and `scripts/optics-release-ref.sh` requires one tag for all five packages.

## Consequences

- `main` must regenerate the canonical `Cargo.lock` against the tagged remote
  sources and pass `cargo check --locked --workspace`; the development
  worktree lockfile remains path-patched and excluded by the commit hook.
- CI clones and builds Optics at
  `eaa66e6a85094bca901d5857776c2360cf9cdbf7` (tag `v0.0.78`).
- `typio-settings` compiles against the record-only Iris API with no behavioural
  change; it inherits the Lens damage localisation, caret phase, and
  terminating ease through Iris.
- The Panel's rendered output is unchanged: transparent clear, full-frame
  repaint, non-blocking acquire, `wl_buffer.release` back-pressure, and
  `damage_buffer` presentation. The pass descriptor now states the whole-extent
  intent explicitly.
- Regional Panel damage remains available but blocked on a documented
  precondition. A future single-buffer or per-buffer-validity pool is the
  trigger to revisit it, not a routine Optics bump.
- Any later Optics update repeats this analysis against the new tag's ADRs. The
  deciding question remains whether the design's premise — a client-timer
  render loop, a rotating swapchain with retained content, a GPU-resident
  coverage source, or retained display-list content — exists in this host.
