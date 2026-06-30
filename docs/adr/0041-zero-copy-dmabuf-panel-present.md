# ADR-0041: Zero-Copy dma-buf Present for the Candidate Panel

- **Status**: Accepted
- **Date**: 2026-06-30
- **Deciders**: Typio maintainers
- **Follows**: [ADR-0040](0040-offscreen-render-shm-buffers.md) (offscreen render + SHM buffers)
- **Amends**: ADR-0040's "sub-millisecond readback" trade-off assessment

## Context

ADR-0040 removed `vkQueuePresentKHR` from the panel path by rendering offscreen
and reading pixels back into CPU memory via `flux_surface_read_pixels`, then
attaching them through a host-managed `wl_shm` buffer. This structurally
eliminated the 16-second WSI present deadlock.

ADR-0040 predicted the per-frame GPU→CPU readback would be a "sub-millisecond
`vkCmdCopyImageToBuffer` + fence wait — negligible compared to the 12–22 ms
`flux_text_draw` cost." Empirical measurement after the glyph-cache fixes
(ADR-0011 / ADR-0019 / ADR-0020) proved this wrong:

| Stage | Measured | ADR-0040 prediction |
|---|---|---|
| `draw_ms` (flux_text + canvas) | **0.12–0.2 ms** | 12–22 ms |
| `readback_ms` (flux_surface_read_pixels) | **14.5–16.6 ms** | sub-millisecond |
| `total_ms` | 12.9–17.2 ms | — |

The trade-off had inverted. With the glyph atlas fully warm
(`glyph_misses_delta=0`, `glyph_hits_delta=20`), GPU rendering dropped to
sub-millisecond, but the readback dominated at 85–95 % of frame time. Every
candidate-page keypress paid a fixed ~15 ms pipeline-stall tax for a fence
synchronisation + staging-buffer copy that moved 1.15 MB of pixels the compositor
never needed on the CPU.

The readback cost is not bandwidth (1.15 MB over PCIe is ~100 µs) — it is the
GPU pipeline drain the fence wait forces, plus the one-shot command buffer
submit for the copy.

## Decision

**Export the offscreen image's GPU memory as a Linux dma-buf and present it
zero-copy via `zwp_linux_dmabuf_v1`.** The compositor composites the GPU memory
directly — no GPU→CPU pixel transfer at all.

The panel pipeline becomes:

```
flux offscreen render (GPU, unchanged)
  → flux_surface_export_dmabuf  (vkGetMemoryFdKHR — fd handle export, µs)
    → host: zwp_linux_buffer_params_v1.create_immed → wl_buffer
      → wl_surface.attach + damage + commit  (same non-blocking path as SHM)
        → compositor composites the dmabuf directly (zero-copy)
```

`flux_surface_read_pixels` and `vkQueuePresentKHR` are both absent from the
dma-buf path. The `wl_buffer` lifecycle is still host-owned (same as ADR-0040's
SHM path), so the deadlock immunity is preserved.

### Implementation

1. **flux offscreen images become exportable.** When the device has the
   external-memory / DRM-modifier extensions, `offscreen_create_images` creates
   images with `VK_IMAGE_TILING_DRM_FORMAT_MODIFIER_EXT` +
   `VkExternalMemoryImageCreateInfo` (handle type
   `VK_EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD_BIT_KHR`) and dedicates their
   `VkDeviceMemory` (dedicated allocation is required for export). The modifier
   is negotiated at creation: prefer `DRM_FORMAT_MOD_LINEAR`, fall back to the
   first modifier supporting colour-attachment + transfer for BGRA8.

2. **`flux_surface_export_dmabuf`** calls `vkGetMemoryFdKHR` on the submitted
   slot's `VkDeviceMemory` after its fence completes, returning a dma-buf fd.
   The stride (`vkGetImageSubresourceLayout` on memory-plane 0) and modifier are
   exposed via `flux_surface_dmabuf_stride` / `flux_surface_dmabuf_modifier`.

3. **Host-side `DmabufBufferPool`** (`panel_dmabuf.rs`). One `wl_buffer` per
   offscreen frame slot, created via `zwp_linux_buffer_params_v1.create_immed`.
   The `wl_buffer.release` registry from ADR-0040 is reused (it is keyed by
   proxy pointer, buffer-type-agnostic). Double-buffered to match
   `frames_in_flight=2`.

4. **Capability-gated with SHM fallback.** If the compositor lacks
   `zwp_linux_dmabuf_v1`, or the device lacks the extensions, or no suitable
   modifier is found, the panel falls back to the ADR-0040 SHM + readback path.
   Both paths coexist; the choice is made at `FluxPanel` construction.

5. **Device extensions** enabled only when dmabuf is active:
   `VK_KHR_external_memory_fd`, `VK_EXT_external_memory_dma_buf`,
   `VK_EXT_image_drm_format_modifier`, `VK_EXT_queue_family_foreign` (device);
   `VK_KHR_external_memory_capabilities`, `VK_KHR_get_physical_device_properties2`
   (instance).

## Consequences

- Positive: The 14.5–16.6 ms readback stall is eliminated on dma-buf-capable
  setups. The only remaining wait in the present path is the fence (GPU render
  completion), which is sub-millisecond when draw is ~0.15 ms.
- Positive: `vkQueuePresentKHR` remains absent (ADR-0040's guarantee holds).
- Positive: SHM fallback preserves correctness on compositors without
  linux-dmabuf support.
- Trade-off: dma-buf export requires the external-memory extensions at device
  creation; compositors that don't advertise `zwp_linux_dmabuf_v1` use the SHM
  path.
- Trade-off: `create_immed` is synchronous; a compositor that rejects the
  format/modifier raises a fatal protocol error. For the trusted local panel
  surface this is acceptable (the fallback is the absence of the global, not a
  runtime reject).
