# ADR-0040: Offscreen Render with Host-Managed SHM Buffers (Remove WSI Present from the Panel)

- **Status**: Accepted
- **Date**: 2026-06-30
- **Deciders**: Typio maintainers
- **Supersedes**: [ADR-0010](0010-non-blocking-candidate-popup-present.md) (non-blocking present mode), [ADR-0013](0013-grow-only-popup-swapchain.md) (grow-only swapchain), [ADR-0036](0036-soft-present-gate-for-candidate-panel.md) (soft present gate)

## Context

The candidate panel rendered to a Vulkan **WSI swapchain** and presented via
`vkQueuePresentKHR`, called synchronously on the input-method event-loop
thread (the only thread). The earlier ADRs in this chain each mitigated a
*symptom* of that architecture but could not remove the root cause:

- **ADR-0010** switched from FIFO to MAILBOX present mode so present would
  not block on vsync. It reduced but did not eliminate blocking: Mesa's
  Wayland WSI dispatches `wl_display` inside `vkQueuePresentKHR`, and that
  internal dispatch can still block waiting for the compositor to recycle
  swapchain images — regardless of present mode.

- **ADR-0013** made the swapchain grow-only (with `wp_viewport` crop) to
  avoid per-page swapchain rebuilds. It eliminated a major source of WSI
  roundtrips, but the swapchain itself remained, and `vkQueuePresentKHR`
  was still in the present path.

- **ADR-0036** turned `wl_surface.frame` into a soft present gate (100 ms
  ceiling) to cap present frequency. It reduced present back-pressure, but
  a **single** `vkQueuePresentKHR` call could still block for 16 seconds
  when the compositor stopped recycling the input-popup's buffers. No
  rate limit can prevent a single present call from blocking — the block
  lives inside the driver, on the caller's thread.

The decisive diagnostic: under rapid Rime candidate paging the event loop
ran cleanly at ~15 ms intervals (dispatch, composition, heartbeat) until
one `vkQueuePresentKHR` call blocked for 16 seconds, tripping the
`Present`-stage watchdog and `SIGKILL`ing the daemon. This was a
**single blocking FFI call**, not present pile-up. Host-side pacing is
powerless against it.

The fundamental problem: the Vulkan WSI owns the buffer lifecycle.
`vkQueuePresentKHR` attaches the swapchain image to the `wl_surface` and
**internally dispatches `wl_display` waiting for `wl_buffer.release`**.
The host never sees `wl_buffer.release` — the WSI intercepts it — so it
cannot bound the wait, cannot time out, and cannot drop a frame. The
buffer-recycling control is welded into a non-interruptible driver call on
the main thread.

## Decision

**Remove the Vulkan WSI swapchain from the panel entirely.** The panel now
renders to a flux **offscreen surface** (`vk_surface_khr = NULL`, no
swapchain, no WSI instance/device extensions) and presents through a
**host-managed `wl_shm` buffer pool** — the same architecture fcitx5 and
ibus use.

The panel pipeline becomes:

```
flux offscreen render (GPU, unchanged: liquid glass, gradients, flux_text)
  → flux_surface_read_pixels (GPU→CPU, bounded by fence timeout, compositor-independent)
    → host: memcpy into a free ShmBuffer (from the host-owned double-buffered wl_shm pool)
      → wl_surface.attach + damage + commit (plain Wayland requests, return instantly)
        → compositor composites; wl_buffer.release reuses the slot (normal event-loop event)
```

`vkQueuePresentKHR` is **no longer invoked anywhere** in the panel path.
The 16-second WSI deadlock is structurally eliminated, not mitigated.

Key implementation points:

1. **Offscreen surface.** `flux_surface_desc.vk_surface_khr = NULL` selects
   flux's offscreen mode (ADR-0013's ADR). The frame loop (`begin_frame` →
   `draw` → `submit` → `present`) is unchanged; for offscreen, `present` is a
   no-op that just rotates the frame slot. `flux_surface_read_pixels` reads
   the result back as tightly packed BGRA8.

2. **No WSI extensions.** The Vulkan device is created without
   `VK_KHR_surface`, `VK_KHR_wayland_surface`, or `VK_KHR_swapchain`. No
   `VkSurfaceKHR` is created. The device is a pure compute/render device.

3. **Host-managed `wl_shm` buffer pool** (`panel_shm.rs`). Double-buffered
   (like fcitx5). `ShmBufferPool::acquire` returns a free buffer or `None`
   (drop this frame — never block). `wl_buffer.release` clears the buffer's
   busy flag via a shared `Arc<Mutex<HashMap>>` registry keyed by proxy
   pointer (not `wl_proxy` user-data, which wayland-client 0.31 owns
   internally for event dispatch).

4. **`wp_viewport` + `buffer_scale = 1`.** The viewport alone maps the
   physical-pixel shm buffer to logical surface coordinates (`set_source` =
   physical crop, `set_destination` = logical size). `buffer_scale` is
   forced to 1 to avoid conflict with viewport coordinate interpretation
   (a `PreferredBufferScale(2)` from the compositor would otherwise shrink
   the compositor's view of the buffer and make the physical source
   rectangle exceed it — protocol error 2).

5. **BGRA8 offscreen format.** The offscreen image uses
   `VK_FORMAT_B8G8R8A8_UNORM`, matching both the windowed swapchain path's
   `pick_format` preference and Wayland's `WL_SHM_FORMAT_ARGB8888` byte
   order (B, G, R, A on little-endian). The host can `memcpy` the readback
   directly into the shm buffer — no per-pixel channel swap.

6. **Present gate retained as pacing hygiene.** The `wl_surface.frame`
   callback gate from ADR-0036 (100 ms ceiling) is kept to avoid spamming
   `wl_surface.attach`/`commit` requests, but it is no longer the primary
   defense against present blocking — there is no present call to block.

## Why not Vulkan WSI at all

The Vulkan WSI (`VK_KHR_swapchain` + `VK_KHR_wayland_surface`) is designed
for **application windows** that own a continuous render loop and a
top-level surface. An input-method popup is fundamentally different:

- **Event-driven, not continuous.** The panel commits a frame only when
  candidates change. FIFO present (built for per-vblank loops) has no
  reason to recycle buffers promptly for a surface it does not schedule.
- **Deprioritized by compositors.** Input-method popups are low-priority
  surfaces; compositors routinely deprioritize, occlude, or delay their
  frame callbacks and buffer releases.
- **Single-threaded.** The input-method event loop must stay responsive to
  keyboard events. A blocking present call freezes input handling. Moving
  present to another thread was tried and rejected: Mesa's WSI dispatches
  `wl_display` inside `vkQueuePresentKHR`, racing the main loop's
  `wl_display_prepare_read` (the maintainers removed the async-present
  thread for this reason).
- **No timeout.** `vkQueuePresentKHR` accepts no timeout parameter. The
  Vulkan spec provides no way to bound it. The only "timeout" is the
  watchdog's `SIGKILL`.

With host-managed shm buffers, all of these constraints vanish: `attach` +
`commit` are non-blocking Wayland requests; `wl_buffer.release` is a normal
event on the host's own queue; if all buffers are busy the host drops the
frame and retries next tick. The compositor's behaviour can never hold the
main thread hostage.

## Alternatives considered

- **Async present thread (revisit).** Move `vkQueuePresentKHR` to a
  dedicated thread so the main loop never blocks. Rejected: Mesa's WSI
  dispatches `wl_display` inside present, racing the main loop's
  `wl_display` access. Solving this requires per-queue isolation deep in
  the WSI (or flux), which is a larger and riskier change than removing WSI
  present entirely.

- **Full CPU rendering (Cairo + Pango + shm).** Abandon Vulkan/flux for the
  panel, like fcitx5. Rejected as the primary path because it discards the
  GPU rendering investment (liquid glass, GPU-accelerated text shaping and
  glyph rasterisation). The offscreen path keeps all of flux's rendering
  and only changes the *present* mechanism. (CPU rendering remains a valid
  fallback if the GPU path proves too heavy for the smallest panels.)

- **Tighter present-rate gating (lower the soft-gate ceiling).** Rejected:
  no rate limit can prevent a single `vkQueuePresentKHR` from blocking for
  16 seconds. The block is in the driver, triggered by compositor state,
  not by call frequency.

- **Lower the watchdog `Present` threshold (3 s → faster kill + restart).**
  Rejected as a primary fix: it converts a 16-second freeze into a shorter
  freeze + restart, but the panel still dies on every rapid-paging burst.
  The watchdog threshold is retained as a genuine-deadlock safety net
  (driver/GPU hang), but it should never fire from compositor back-pressure
  again.

## Consequences

- Positive: The `vkQueuePresentKHR` deadlock is **structurally gone**.
  `vkQueuePresentKHR` is not in the panel's code path. A compositor that
  stops recycling buffers causes at most dropped frames, never a freeze or
  process kill.
- Positive: The event loop is fully responsive during panel rendering.
  `attach`/`commit` return instantly; `flux_surface_read_pixels` is bounded
  by a GPU fence timeout.
- Positive: All GPU rendering (liquid glass, gradients, `flux_text`) is
  preserved unchanged — only the present mechanism changed.
- Positive: The buffer lifecycle is under host control. `wl_buffer.release`
  is a normal event the host processes on its own queue, exactly as fcitx5
  and ibus do.
- Trade-off: One GPU→CPU readback per frame (`flux_surface_read_pixels`).
  For a typical candidate panel (~1–2 MB) this is a sub-millisecond
  `vkCmdCopyImageToBuffer` + fence wait — negligible compared to the
  12–22 ms `flux_text_draw` cost for new CJK glyphs.
- Trade-off: The panel now depends on `wl_shm` (baseline Wayland, always
  available) and adds a small amount of shm-buffer management code
  (`panel_shm.rs`).
- Negative (accepted): ADR-0010's MAILBOX present mode, ADR-0013's
  swapchain sizing, and ADR-0036's present gate are no longer the primary
  defense. The `wp_viewport` grow-only sizing from ADR-0013 is retained
  (now applied to the offscreen image instead of a swapchain). The
  frame-callback gate from ADR-0036 is retained as pacing hygiene.
