# How to Diagnose Candidate-Switching Lag

Use this guide when typing remains correct but candidate selection or paging
looks delayed, skips frames, or catches up in bursts.

## Current pipeline

```text
key → engine → latest CompositionState
    → Panel driver (DIRTY convergence + presentation deduplication)
    → flux CPU canvas + TextRaster
    → host-owned wl_shm buffer → Wayland compositor
```

Inline preedit is a separate path: Typio sends
`zwp_input_method_v2.set_preedit_string`, and the focused application renders
the text. A stale inline letter is therefore not a Panel rendering problem; see
[Wayland Input Method](../explanation/wayland-input-method.md).

The candidate path has no Vulkan swapchain, GPU glyph atlas, dma-buf export, or
`wl_surface.frame` pacing. SHM buffer availability is its only presentation
back-pressure mechanism.

## Capture a focused trace

First confirm that the native CPU renderer is optimized. Current Cargo release
builds reject a Meson tree that reports optimization level zero, but an older
daemon can keep an unoptimized library mapped after the build tree is replaced:

```bash
meson configure "${FLUX_BUILD_DIR:-../optics/build-release}" | rg "buildtype|optimization"
pid="$(pgrep -o typio)"
rg '/optics/.*/libflux' "/proc/$pid/maps"
```

Use `buildtype=release` for the trace. [How to Package for
Distribution](package-for-distribution.md#build-a-release-binary) shows the
separate native release build tree. If the running daemon predates that build,
its `maps` entry can end in `(deleted)`; the new binary and libraries take
effect only in a later session. Then capture:

```bash
cargo build --release -p typio-host --bin typio
RUST_LOG="typio.engine.key=trace,typio.panel.scheduler=trace,typio.panel.perf=trace,typio.panel.shm=debug" \
  ./target/release/typio 2>&1 | tee typio-panel.log
```

For a daemon already running at the default log level, send `SIGUSR1` while
reproducing and `SIGUSR2` afterward. The global level change is less focused
than the target filter above, but it avoids restarting an intermittent session.

## Read the trace in order

### 1. Engine latency

`typio.engine.key` reports every `process_key` call at trace level and promotes
calls slower than 5 ms to info. If latency appears here before a composition
sequence changes, the engine is the bottleneck rather than Panel presentation.

### 2. Scheduler convergence

`typio.panel.scheduler` shows the current `composition_seq`, candidate count,
selection, focus, and context. A dirty snapshot should either:

- settle because focus or context disappeared;
- hide because candidates became empty;
- skip because that sequence is already visible;
- present successfully; or
- remain dirty after an SHM frame drop so a later event retries the newest
  snapshot.

Repeated retries for one sequence should correspond to SHM exhaustion. A
sequence that changes logically without any scheduler line points upstream at
composition delivery or logging configuration.

### 3. CPU render and SHM attach

`typio.panel.perf` splits a candidate frame into `layout_us`, `acquire_us`,
`draw_us`, and `present_us`. The nested `present_shm` event
further reports `read_pixels_us`, `swap_us`, and `attach_us`.

- High `layout_us` or `draw_us` points at text measurement/rasterisation.
- High `read_pixels_us` points at the 2×2 supersample resolve.
- High `swap_us` points at the RGBA-to-ARGB framebuffer copy.
- High `attach_us` points at the Wayland attach/commit call.
- `reason=shm_unavailable` with `shm buffer pool exhausted` means the compositor
  still owns every buffer. The driver skips CPU drawing and retains only the
  latest dirty snapshot; it never blocks key processing for a release.

### 4. Viewporter fallback

Without `wp_viewporter`, every content-size change requires an exact-size CPU
canvas and SHM buffer instead of reusing a quantized, hysteretically sized
backing surface. Check:

```bash
wayland-info | rg -i viewporter
journalctl --user -u typio | rg "wp_viewporter"
```

This produces a roughly constant resizing cost, not latency that grows over
time.

## Decision table

| Evidence | Likely owner |
|---|---|
| `process_key` exceeds 5 ms | engine |
| scheduler sees no new composition sequence | engine/composition callback |
| high `layout_us` or `draw_us` | text layout/rasterisation |
| repeated SHM exhaustion | compositor buffer-release scheduling |
| high `read_pixels_us` | CPU supersample resolve |
| high `swap_us` | CPU framebuffer conversion |
| constant resize cost and no `wp_viewporter` | compositor capability fallback |

## Bug report checklist

- `typio --version` and build profile;
- compositor name and version;
- a trace spanning the slow interaction;
- any `shm buffer pool exhausted` lines;
- the `wayland-info` viewporter result;
- whether the lag affects inline preedit, candidates, or both;
- a runtime-state snapshot if focus or grab state looks suspicious.

## Related

- [Candidate Panel Behavior](../explanation/candidate-panel-behavior.md)
- [Event Loop Scheduling](../explanation/event-loop-scheduling.md)
- [Performance & Idle-Power Strategy](../explanation/performance-strategy.md)
- [ADR-0040: CPU Canvas Render with Host-Managed SHM Buffers](../adr/0040-cpu-canvas-render-shm-buffers.md)
