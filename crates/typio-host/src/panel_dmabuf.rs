//! Host-managed dma-buf buffer pool for the candidate panel.
//!
//! Zero-copy counterpart to [`crate::panel_shm`]. Instead of reading GPU pixels
//! back into CPU memory (the 12–16 ms `flux_surface_read_pixels` fence stall),
//! flux exports the offscreen image's `VkDeviceMemory` as a Linux dma-buf fd,
//! and this module wraps it as a `wl_buffer` via `zwp_linux_buffer_params_v1`.
//! The compositor composites the GPU memory directly — no GPU→CPU round trip.
//!
//! ## Synchronisation model
//!
//! The surface has `frames_in_flight` offscreen images (== frame slots). Each
//! slot's backing `VkDeviceMemory` is exported to a dma-buf fd once at buffer
//! creation; the fd lives as long as the `wl_buffer` does. `flux_surface_export_dmabuf`
//! is called per-frame to get a *fresh* fd for `last_submitted_slot`, but the
//! `wl_buffer`/fd is reused — the compositor reads the same memory the GPU just
//! wrote, gated by the export's fence wait.
//!
//! `busy` reuses the same `ShmReleaseRegistry` (keyed by `wl_buffer` proxy
//! pointer, buffer-type-agnostic). `wl_buffer.release` clears it exactly as in
//! the SHM path.
//!
//! Design mirrors [`crate::panel_shm`]: double-buffered, non-blocking acquire,
//! release-driven reuse.

use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use wayland_client::protocol::wl_buffer;
use wayland_client::{Dispatch, Proxy, QueueHandle};

use crate::input_method::InputMethodState;
use crate::protocols::linux_dmabuf_v1::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1};
use crate::panel_shm::ShmReleaseRegistry;

/// DRM format code for `DRM_FORMAT_ARGB8888` — fourcc 'AR24'.
/// Matches flux's `VK_FORMAT_B8G8R8A8_UNORM` offscreen format (BGRA byte
/// order == Wayland ARGB8888 on little-endian; see ADR-0040 point 5).
const DRM_FORMAT_ARGB8888: u32 = 0x34325241; // 'AR24' in little-endian

/// One dma-buf-backed `wl_buffer` wrapping a GPU offscreen image slot.
///
/// The fd is obtained from flux via `flux_surface_export_dmabuf`; it references
/// the slot's `VkDeviceMemory` (the same memory the GPU renders into). The
/// `wl_buffer` is created with `create_immed` (synchronous; the compositor
/// imports immediately or raises a fatal error — acceptable for a trusted
/// local surface). `busy` tracks compositor ownership via `wl_buffer.release`.
pub struct DmabufBuffer {
    /// Owned dma-buf fd. Kept alive for the buffer's lifetime: the compositor
    /// may re-import it at any time until `wl_buffer` is destroyed.
    _fd: OwnedFd,
    buffer: wl_buffer::WlBuffer,
    width: u32,
    height: u32,
    busy: Arc<AtomicBool>,
    registry: ShmReleaseRegistry,
    key: usize,
}

impl DmabufBuffer {
    /// Create a dma-buf `wl_buffer` from an exported fd.
    ///
    /// `fd` is the dma-buf fd from `flux_surface_export_dmabuf` (duplicated;
    /// this takes ownership). `modifier` is the DRM modifier the image was
    /// created with; `stride` is the row pitch in bytes. `plane_offset` is
    /// the byte offset of plane 0 within the dma-buf (0 for our single-plane
    /// BGRA8 images).
    pub fn new(
        dmabuf: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        qh: &QueueHandle<InputMethodState>,
        registry: &ShmReleaseRegistry,
        fd: OwnedFd,
        width: u32,
        height: u32,
        stride: u32,
        modifier: u64,
    ) -> Self {
        let params = dmabuf.create_params(qh, ());

        // Single-plane BGRA8: one plane at offset 0.
        params.add(
            fd.as_fd(),
            0, // plane index
            0, // offset within dmabuf
            stride,                 // row pitch
            (modifier >> 32) as u32, // modifier high 32 bits
            modifier as u32,         // modifier low 32 bits
        );

        // create_immed: the compositor imports synchronously. On failure it
        // either sends `failed` or raises a fatal error. For our trusted
        // local panel surface, a fatal error here means a driver/compositor
        // incompatibility — the SHM fallback path is the safety net.
        let buffer = params.create_immed(
            width as i32,
            height as i32,
            DRM_FORMAT_ARGB8888,
            zwp_linux_buffer_params_v1::Flags::empty(),
            qh,
            (),
        );

        let busy = Arc::new(AtomicBool::new(false));
        let key = buffer.id().as_ptr() as usize;
        registry.lock().unwrap().insert(key, busy.clone());

        Self {
            _fd: fd,
            buffer,
            width,
            height,
            busy,
            registry: registry.clone(),
            key,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn wl_buffer(&self) -> &wl_buffer::WlBuffer {
        &self.buffer
    }

    pub fn mark_busy(&self) {
        self.busy.store(true, Ordering::Release);
    }
}

impl Drop for DmabufBuffer {
    fn drop(&mut self) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.remove(&self.key);
        }
        // buffer drop sends wl_buffer.destroy; _fd closes the dmabuf fd.
        // Safe here because this buffer only reaches `drop` once
        // `busy == false` (the compositor has sent `wl_buffer.release`):
        //   - resize/clear/ensure_slot park busy buffers in `pending_release`
        //     and reap them only after release flips the flag, and
        //   - teardown runs `flux_surface_release` → `vkDeviceWaitIdle` first,
        //     which also drains the compositor's last frame.
    }
}

/// A pool of dma-buf buffers for the panel, one per offscreen frame slot.
///
/// Unlike the SHM pool (which grows on demand up to a cap), this pool is sized
/// to the surface's `frames_in_flight`: each buffer wraps a distinct image
/// slot's dma-buf, so there's exactly one buffer per slot. The panel exports
/// the freshly-submitted slot's dma-buf each frame and re-binds it to the
/// pre-existing buffer (the wl_buffer/fd pair is stable for a given slot as
/// long as the surface isn't resized).
pub struct DmabufBufferPool {
    /// One buffer per frame slot, index-aligned with flux's image slots.
    buffers: Vec<Option<DmabufBuffer>>,
    /// Buffers retired from `buffers` but still awaiting compositor `release`
    /// before they may be destroyed. Populated by `clear()` (resize) and by
    /// `ensure_slot` (slot recreated mid-flight) when the displaced buffer is
    /// still busy; drained by the reap pass at the top of `ensure_slot` as
    /// releases arrive.
    ///
    /// Destroying a `wl_buffer` the compositor has not released is a protocol
    /// violation, and unlike teardown this path runs no `vkDeviceWaitIdle` —
    /// so busy buffers are parked here rather than dropped in place.
    pending_release: Vec<DmabufBuffer>,
    dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    qh: QueueHandle<InputMethodState>,
    registry: ShmReleaseRegistry,
}

impl DmabufBufferPool {
    pub fn new(
        dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        qh: QueueHandle<InputMethodState>,
        registry: ShmReleaseRegistry,
    ) -> Self {
        Self {
            buffers: Vec::new(),
            pending_release: Vec::new(),
            dmabuf,
            qh,
            registry,
        }
    }

    /// Ensure the pool has a buffer for frame `slot`, creating one from the
    /// given dma-buf fd if needed (first use or post-resize). Returns the
    /// slot index on success.
    pub fn ensure_slot(
        &mut self,
        slot: usize,
        fd: OwnedFd,
        width: u32,
        height: u32,
        stride: u32,
        modifier: u64,
    ) {
        // Reap retired buffers whose compositor release has arrived since the
        // last call. Each `wl_buffer.release` flips its `busy` flag, so a
        // released entry is now safe to destroy. Reap happens here (a frame
        // boundary) so we never tear down a wl_buffer mid-event-queue.
        self.pending_release.retain(|b| {
            if b.busy() {
                true // still held — keep parking
            } else {
                tracing::trace!(
                    target: "typio.panel.dmabuf",
                    "reaped retired dmabuf buffer after compositor release"
                );
                false // drop in place — release already happened
            }
        });

        // Grow the vec to fit the slot index.
        if self.buffers.len() <= slot {
            self.buffers.resize_with(slot + 1, || None);
        }
        let existing = self.buffers[slot].take();
        let needs_create = match &existing {
            None => true,
            Some(b) => b.width() != width || b.height() != height,
        };
        if needs_create {
            // The displaced buffer, if any, must not be destroyed while the
            // compositor still holds it. Retire it to `pending_release` if it's
            // busy; otherwise it's already free and can be dropped normally.
            if let Some(b) = existing {
                if b.busy() {
                    tracing::trace!(
                        target: "typio.panel.dmabuf",
                        slot,
                        "retiring busy dmabuf buffer to pending_release on re-create"
                    );
                    self.pending_release.push(b);
                }
                // A free or non-busy buffer simply falls out of scope here.
            }
            self.buffers[slot] = Some(DmabufBuffer::new(
                &self.dmabuf,
                &self.qh,
                &self.registry,
                fd,
                width,
                height,
                stride,
                modifier,
            ));
        } else {
            // Matches current size — reuse the existing buffer. The fresh `fd`
            // from this frame's export is surplus (the slot's permanent fd
            // already lives inside the kept buffer); it drops here.
            self.buffers[slot] = existing;
        }
    }

    /// Get the buffer for a slot, marking it busy for the compositor.
    pub fn get(&self, slot: usize) -> Option<&DmabufBuffer> {
        self.buffers.get(slot).and_then(|b| b.as_ref())
    }

    /// True if the given slot's buffer is still held by the compositor.
    pub fn is_busy(&self, slot: usize) -> bool {
        self.buffers
            .get(slot)
            .and_then(|b| b.as_ref())
            .map(|b| b.busy())
            .unwrap_or(false)
    }

    /// Retire all buffers on resize. Buffers the compositor has already
    /// released are dropped immediately; any still busy (compositor may still
    /// be compositing/scanout) are parked in `pending_release` and reaped by
    /// `ensure_slot` once their `wl_buffer.release` arrives.
    ///
    /// This is the resize path — unlike teardown it runs no
    /// `vkDeviceWaitIdle`, so we cannot tear down a buffer the compositor has
    /// not released.
    pub fn clear(&mut self) {
        for slot in self.buffers.iter_mut() {
            if let Some(b) = slot.take() {
                if b.busy() {
                    tracing::trace!(
                        target: "typio.panel.dmabuf",
                        "retiring busy dmabuf buffer to pending_release on clear"
                    );
                    self.pending_release.push(b);
                }
                // Non-busy buffers fall out of scope here — already released.
            }
        }
        self.buffers.clear();
    }
}

// ── Wayland dispatch ──────────────────────────────────────────────────────
//
// zwp_linux_dmabuf_v1 and zwp_linux_buffer_params_v1 events we handle:
// - dmabuf: `format` / `modifier` (v3) advertising support — ignored, we use
//   create_immed and let the compositor reject if unsupported.
// - buffer_params: `created` / `failed` — only for the async `create` path;
//   we use `create_immed` which delivers the wl_buffer directly, so these
//   never fire. We still must impl Dispatch to satisfy wayland-client routing.
// - wl_buffer: `release` — reused from panel_shm (keyed by proxy pointer).

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        _event: <zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1 as Proxy>::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // format / modifier advertisement — ignored (create_immed path).
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        _event: <zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1 as Proxy>::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // created / failed — only for async create(); create_immed is sync.
    }
}
