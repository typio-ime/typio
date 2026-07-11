# ADR-0044: Bounded Panel Rendering Before SHM Presentation

- **Status**: Accepted
- **Date**: 2026-07-12
- **Deciders**: Project maintainers

## Context

The CPU-canvas Panel removed blocking Vulkan presentation, but three costs
still made candidate navigation visibly uneven:

- the documented build commands produced an unoptimized Meson `debug`
  `libflux`, even when the Rust daemon used Cargo's release profile;
- the framebuffer was grow-only, so one unusually wide candidate page made
  every later clear, 2×2 supersample resolve, and SHM copy process that
  historical maximum extent;
- the renderer completed the CPU frame before asking the SHM pool for a free
  buffer, wasting the full render cost when compositor back-pressure required
  the frame to be skipped.

The latest-snapshot scheduler was already correct: a skipped frame keeps the
Panel dirty, and a later `wl_buffer.release` reactor step retries the newest
composition. The problem was work performed before that convergence point,
not candidate ordering.

## Decision

The Panel rendering boundary follows these rules:

- production builds link a Meson `release` optics tree; contributor and CI
  builds use `debugoptimized`, never optimization level zero;
- the CPU canvas and SHM buffers are allocated lazily from the first real
  content extent;
- with `wp_viewporter`, width and height remain quantized to 64×32 pixels,
  grow when required, and shrink when current content needs no more than half
  of an axis; without viewporter, the extent remains exact;
- a free SHM buffer is reserved before canvas creation, clearing, text
  rasterization, or supersample resolve;
- when all three buffers are busy, no frame is rendered, the dirty state is
  retained, and the next release/input step retries only the latest snapshot;
- full-frame repaint remains the rendering model. Partial highlight damage is
  deferred until measurements show it is necessary because it would add a
  second cached visual representation and invalidation protocol.

This supersedes the indefinitely grow-only sizing part of ADR-0013 and the
retained grow-only note in ADR-0040. Their non-blocking SHM presentation
decisions remain in force.

## Alternatives considered

- **Keep a permanently grow-only canvas**: rejected because CPU clear and
  resolve cost scales with allocated pixels, unlike the retired GPU
  swapchain where avoiding recreation dominated.
- **Resize to every exact candidate width**: rejected because adjacent pages
  often differ slightly and would churn CPU canvases and SHM objects.
- **Render, then check SHM availability**: rejected because the result is
  guaranteed to be discarded when the pool is busy.
- **Restore `wl_surface.frame` pacing**: rejected because buffer release already
  provides back-pressure, while frame callbacks previously caused visible
  stalls when missing.
- **Immediately implement partial highlight redraw**: deferred because the
  optimized, bounded full-frame path is simpler and keeps one authoritative
  framebuffer.

## Consequences

- Positive: Cargo release builds no longer silently run the Panel's native hot
  path at optimization level zero.
- Positive: rendering cost follows recent content size rather than the widest
  page seen since startup.
- Positive: compositor back-pressure consumes no text or canvas work until a
  buffer is available.
- Positive: candidate ordering, latest-state coalescing, and non-blocking input
  behavior remain unchanged.
- Trade-off: crossing the half-capacity threshold reallocates a canvas and, as
  buffers become free, matching SHM objects.
- Trade-off: selection-only changes still repaint the bounded full frame.
