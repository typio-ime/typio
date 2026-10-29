# ADR-0053: Optics v0.0.62 adoption boundary

- **Status**: Accepted
- **Date**: 2026-10-29
- **Scope**: performance
- **Deciders**: Typio maintainers
- **Related RFC**: Not used — this repository does not operate an RFC workflow

## Context

The workspace pins five Optics bindings (`flux-sys`, `glyph-sys`, `iris`,
`iris-sys`, `lens-sys`) to one release tag, and `scripts/optics-release-ref.sh`
enforces that all five move together. The previous pin, Optics `v0.0.52`, is ten
patch releases behind `v0.0.62`. Between those tags Optics shipped a large body
of design work — display-paced presentation, incremental damage, layer
decoupling, a dual-thread presentation engine, icon coverage masks, and a
pre-fold event interception seam — recorded as Optics ADR-0107 and
ADR-0110 through ADR-0124.

The migration is not a routine dependency bump, because the upstream release
notes describe designs that a downstream host is expected to *adopt*, not merely
link against. The open question this record settles is therefore not "which tag"
but **which of those designs Typio should adopt, and which it must deliberately
decline**, so that a later reader does not re-open the question from scratch or,
worse, adopt a design that is structurally wrong for this host.

Typio's Panel is the deciding constraint. It renders on the **CPU** into a
`flux_canvas_create_cpu` canvas, byte-swaps the premultiplied RGBA8 result into a
host-owned `wl_shm` buffer, and attaches it through raw `wl_surface` FFI on the
single reactor thread. It is event-driven: it repaints when a composition
sequence changes (ADR-0023, ADR-0044), not on a frame clock. Typio never creates
an Optics `iris` application for its Panel, never owns a Vulkan swapchain for it,
and has no continuous animation.

## Decision

Adopt Optics `v0.0.62` as the single release pin, and adopt only those upstream
designs whose premises hold for a CPU-canvas, `wl_shm`, event-driven host:

1. **Adopt `glyph_draw_outlined` for tray language badges.** The API existed at
   `v0.0.52` but was unused. It dilates glyph coverage over eight directions
   with normalised diagonals *before* the foreground pass, so the outline cannot
   darken the glyph. This replaces a hand-rolled nine-call ring that re-ran
   layout once per offset per badge size.
2. **Adopt the pin, lockfile, and CI reference as one atomic unit.** The tag,
   the resolved commit, and `OPTICS_PINNED_REF` move together, because the
   `-sys` build scripts floor `pkg-config` at `0.0.30` and will therefore link a
   stale system install without complaint.
3. **Decline display-paced presentation (Optics ADR-0122) for the Panel.** It
   solves a problem Typio does not have: a render loop paced by a client timer.
4. **Decline incremental damage (Optics ADR-0123) as a partial-damage present.**
5. **Decline icon/supersample coverage baking (Optics ADR-0112) for text
   badges.** Supersampling raises the coverage resolution of *geometry*, not of
   glyphs, which are sourced from the FreeType R8 atlas.
6. **Decline the dual-thread presentation engine (Optics ADR-0124) and the
   pre-fold event seam (Optics ADR-0107).** Neither is reachable from the
   Rust surface, and the event seam belongs to a host that owns the event pump.

## Rationale

**Why `glyph_draw_outlined` is a genuine win.** It is the one upstream design
whose premises match Typio exactly, and the code it replaces was strictly worse.
The previous badge path issued nine `glyph_draw` calls per size: eight offset
copies of the glyph in black, then the foreground. Every call rebuilt the text
layout, so a two-glyph badge at three icon sizes performed twenty-seven layout
builds where three suffice. The offsets were also a non-normalised ±1 px square
ring, so the contour widened by √2 at the corners rather than staying at the
requested radius. Adopting the upstream call removes both defects and keeps the
delivered ink footprint unchanged at the chosen width.

**Why display pacing does not apply.** Optics ADR-0122 establishes
`[INV-PACE-01]` — no client-timer presentation — against a backend that slept
out a hardcoded period and then re-derived its next wake-up from the last paint.
Typio's Panel never had that shape: the poll timeout is unbounded by default and
is folded down only by real deadlines, and `INV-LOOP-03` already forbids a
periodic baseline tick. Adopting `wl_surface.frame` subscription would
*reintroduce* a mechanism this repository deliberately retired (ADR-0044,
ADR-0036): presentation here is back-pressure driven, and the reserve-before-render
rule (`INV-PANEL-08`) plus `wl_buffer.release` is the cadence authority. There is
no animation to smooth, so there is nothing for a frame clock to pace.

**Why partial damage is unsafe here.** Optics ADR-0123 constrains the damage
region to the dirty bounding box and relies on the painter covering everything
inside that box. Typio's model is the opposite: the full frame is repainted,
because `ShmBufferPool::acquire` returns the **first free buffer** from a pool of
three, not the next buffer in rotation. Successive frames therefore land in
different buffers whose contents diverge, and a compositor may recycle a buffer
whose pixels are several frames stale. Damage smaller than the frame would let
the stale remainder show through. Making partial damage correct requires either a
single-buffer pool or a retained, per-buffer validity record — a redesign that
would trade away the non-blocking present path for a per-keystroke win that the
bounded extent policy (ADR-0044) already keeps small.

**Why supersampling does not sharpen text badges.** `flux_canvas_create_cpu_ss`
raises the *canvas* sample count, and `flux_canvas_cpu_coverage` reports the
box-filtered mean of the supersampled alpha channel. That is a meaningful lift
for vector geometry. A text badge, however, draws through `glyph_draw`, which
sources coverage from the context's FreeType R8 atlas at one sample per device
pixel; the canvas sample buffer is downstream of that choice and cannot recover
resolution the atlas never produced. Measured on the `EN` badge, the distinct
coverage levels are identical at every factor from 1 through 8. A geometry fill
over the same range rises from 20 to 37 levels, which isolates the cause. Baking
supersampled masks would raise memory by up to 64 float samples per pixel and buy
nothing visible.

**Why the pin is one atomic unit.** Each `-sys` build script floors
`pkg-config` at `0.0.30`, far below any tag this repository has pinned, so a
stale system `libflux`/`liblens`/`libiris` satisfies the probe silently. The
lockfile, the workspace tag, and `OPTICS_PINNED_REF` are only correct together;
a partially advanced pin produces a bindgen surface that disagrees with the
linked library.

## Rejected Alternatives

- **Track `v0.0.52` — no change.** Rejected: the pin is ten patches behind, the
  badge rasteriser keeps a demonstrable defect (twenty-seven layout builds and a
  non-normalised outline for a three-size badge), and the stale-`pkg-config`
  hazard only grows the further the pin falls behind the installed C stack.
- **Adopt `wl_surface.frame` pacing for the Panel.** Rejected: it reverses
  ADR-0044 and ADR-0036 without addressing a real defect, and it would put a
  frame clock in a path whose only trigger is a changed composition sequence.
  Reintroducing a retired mechanism is a regression, not an upgrade.
- **Adopt partial damage with `flux_canvas_pass_desc::render_offset_*` plus
  `lens_damage_rect`.** Rejected for now: with a three-buffer pool that returns
  the first free buffer, the untouched region of the newly acquired buffer is not
  guaranteed to hold the previously presented pixels, so the compositor can
  composite stale content. The correctness precondition — retained per-buffer
  contents — is a present-path redesign, and the bounded extent policy already
  keeps a full repaint cheap. This is recorded as a *conditional* rejection: it
  becomes viable if the Pool is pinned to a single buffer or gains per-buffer
  validity tracking.
- **Bake badge masks with `flux_canvas_create_cpu_ss(..., 8)` and blit 1:1.**
  Rejected: measured to produce no additional coverage levels for glyph text,
  because the R8 atlas — not the canvas sample buffer — sets the resolution.
- **Use `iris_paint_mark_static` / `iris_request_frame_skip_render` to quiesce
  the settings window.** Rejected: both are defined relative to a paint
  callback, and both are no-ops without one. `typio-settings` passes
  `None::<fn(PaintHost)>`, so the declarations would be inert while reading as
  if they did something.
- **Reach through `iris_sys` to wire the `on_event` seam.** Rejected: it is
  already an internal seam that requires owning the struct literal, and it
  duplicates a policy-bearing internal path without a consumer need. If input
  interception is ever required, it should be adopted as a supported binding or
  not at all.
- **Adopt the dual-thread presentation engine.** Rejected: `iris_frame_mailbox`
  is an internal header that is not installed and not bound, and the
  `[INV-THREAD-03]` swapchain-ownership rule has no subject in a `wl_shm` host.
  The transferable part — never block input on presentation — Typio already
  satisfies through the non-blocking acquire.

## Consequences

- The Panel's present path is unchanged: full-frame repaint, non-blocking
  acquire, `wl_buffer.release` back-pressure. The retirement of `wl_surface.frame`
  as a present gate stands.
- The full-frame repaint is only correct if the pass actually resets the
  framebuffer. It previously did not: the pass was opened with a null descriptor,
  which *loads* rather than clears, so a shrinking panel kept the previous
  frame's pixels and its corner arcs blended over the new body's corners. That
  defect is exactly the failure mode judgement 4 declines to invite: with
  three rotating buffers and no retained validity, anything that is not repainted
  is composited from stale contents. Clearing to transparent is the cheap
  invariant that makes the full repaint self-contained. See
  [Panel Rendering](../architecture/panel-rendering.md).
- Badge rasterisation is cheaper and its outline geometry is now normalised.
  Delivered ink coverage was verified unchanged at the selected width.
- The `pkg-config` floor remains a live hazard and is now documented here: a
  stale system Optics install is satisfied by `0.0.30` and links silently. The
  lockfile plus `OPTICS_PINNED_REF` are the guard, not the probe.
- Partial damage stays available but blocked on a documented precondition. A
  future change that pins the SHM pool to one buffer, or that records per-buffer
  contents, is the trigger to revisit it.
- Any later Optics bump repeats this analysis against the new tag's ADRs. The
  deciding question is always whether the design's premise — a client-timer
  render loop, a rotating swapchain, a GPU-resident coverage source — exists in
  this host.
