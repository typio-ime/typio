//! Host-managed SHM buffer pool for the candidate panel.
//!
//! Replaces the Vulkan WSI swapchain present path. flux renders offscreen,
//! `flux_surface_read_pixels` reads the frame back into CPU memory, and this
//! module hands it to the compositor via a `wl_shm` `wl_buffer`. The host
//! owns the buffer lifecycle: `wl_buffer.release` is a normal event on the
//! host's own queue, so a compositor that stops recycling buffers can at
//! worst cause dropped frames — never the 16 s `vkQueuePresentKHR` deadlock
//! the WSI path suffered.
//!
//! Design mirrors fcitx5's `Buffer` / `WaylandShmWindow` (double-buffered,
//! non-blocking `acquire`, `release`-dranced reuse), translated to Rust +
//! wayland-client 0.31.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::{Dispatch, Proxy, QueueHandle};

use crate::input_method::InputMethodState;

/// ARGB8888: 4 bytes per pixel (Wayland's mandatory baseline format).
const BYTES_PER_PIXEL: usize = 4;

/// Shared registry mapping a `wl_buffer` proxy pointer to its busy flag.
/// Used by the `Dispatch<wl_buffer>` release handler to clear the right
/// buffer's busy flag without `wl_proxy` user-data (which wayland-client
/// 0.31 owns internally for event routing). Cloned (cheap Arc clone) into
/// each `FluxPanel` so it can register buffers at creation time.
pub type ShmReleaseRegistry = Arc<Mutex<HashMap<usize, Arc<AtomicBool>>>>;

pub fn new_release_registry() -> ShmReleaseRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

/// One SHM-backed `wl_buffer` with an mmap'd pixel region.
///
/// `busy` tracks whether the compositor still holds this buffer (it was
/// attached+committed and `wl_buffer.release` has not yet arrived). The flag
/// is `Arc`-shared with the release registry so the `Dispatch<wl_buffer>`
/// handler can clear it.
pub struct ShmBuffer {
    mapping: ManuallyDrop<ShmMapping>,
    pool: ManuallyDrop<wl_shm_pool::WlShmPool>,
    buffer: ManuallyDrop<wl_buffer::WlBuffer>,
    width: u32,
    height: u32,
    busy: Arc<AtomicBool>,
    /// The release registry this buffer registered itself in, plus the key it
    /// used (`wl_buffer` proxy pointer). Held so `Drop` can deregister and
    /// keep the map from accumulating stale entries across pool reallocations.
    registry: ShmReleaseRegistry,
    key: usize,
}

/// Owned `mmap` region over an anonymous `memfd`/`tmpfile`. Unmapped + fd
/// closed on drop.
struct ShmMapping {
    ptr: *mut u8,
    len: usize,
    _fd: OwnedFd,
}

impl ShmMapping {
    fn alloc(len: usize) -> Result<Self, ShmError> {
        // Prefer memfd_create (Linux, sealed against shrink); fall back to
        // O_TMPFILE under XDG_RUNTIME_DIR (Wayland compositors may require
        // the fd to live on a tmpfs the compositor can mmap). The final
        // mkstemp fallback covers non-tmpfs XDG_RUNTIME_DIR edge cases.
        let fd = open_shm_fd(len)?;
        // fallocate ensures the file is actually `len` bytes before mmap
        // (avoids SIGBUS if the compositor reads past a sparse tail).
        if unsafe { libc::fallocate(fd.as_raw_fd(), 0, 0, len as i64) } != 0 {
            // Fall back to ftruncate on filesystems that don't support
            // fallocate (e.g. some tmpfs configurations).
            if unsafe { libc::ftruncate(fd.as_raw_fd(), len as libc::off_t) } != 0 {
                return Err(ShmError::Fallocate(std::io::Error::last_os_error()));
            }
        }
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(ShmError::Mmap(std::io::Error::last_os_error()));
        }
        Ok(Self {
            ptr: ptr as *mut u8,
            len,
            _fd: fd,
        })
    }

    fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for ShmMapping {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { libc::munmap(self.ptr as *mut c_void, self.len) };
        }
    }
}

unsafe impl Send for ShmMapping {}
unsafe impl Sync for ShmMapping {}

/// Create an anonymous shared-memory fd, sized for `len` bytes.
fn open_shm_fd(len: usize) -> Result<OwnedFd, ShmError> {
    // 1. memfd_create with the sealable flag (best on modern Linux).
    let name = c"typio-panel-shm";
    let fd = unsafe { libc::syscall(libc::SYS_memfd_create, name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd >= 0 {
        let owned = unsafe { OwnedFd::from_raw_fd(fd as RawFd) };
        // Best-effort seal against shrink; not fatal if it fails (some
        // kernels/configs lack F_ADD_SEALS on memfd).
        let _ = nix::fcntl::fcntl(
            &owned,
            nix::fcntl::FcntlArg::F_ADD_SEALS(nix::fcntl::SealFlag::F_SEAL_SHRINK),
        );
        return Ok(owned);
    }

    // 2. O_TMPFILE under XDG_RUNTIME_DIR.
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        let path = std::ffi::CString::new(format!("{xdg}/.")).unwrap();
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd >= 0 {
            return Ok(unsafe { OwnedFd::from_raw_fd(fd) });
        }
    }

    // 3. mkstemp fallback.
    let tmpl = {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        format!("{base}/typio-shm-XXXXXX")
    };
    let c_tmpl = std::ffi::CString::new(tmpl.as_bytes()).unwrap();
    let fd = unsafe { libc::mkstemp(c_tmpl.as_ptr() as *mut libc::c_char) };
    if fd < 0 {
        return Err(ShmError::OpenFd(std::io::Error::last_os_error()));
    }
    // Unlink immediately so the name goes away; the fd lives until dropped.
    unsafe { libc::unlink(c_tmpl.as_ptr()) };
    // Ensure the file is at least `len` before mmap (mkstemp gives an empty
    // file; fallocate in alloc handles the rest, but a pre-truncate avoids a
    // sparse edge on filesystems that reject fallocate).
    let _ = unsafe { libc::ftruncate(fd, len as libc::off_t) };
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

impl ShmBuffer {
    /// Create a new SHM buffer of `width × height` (physical px, ARGB8888).
    ///
    /// `shm` is the bound `wl_shm` global. `qh` is the queue that will receive
    /// the `release` event.
    pub fn new(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<InputMethodState>,
        registry: &ShmReleaseRegistry,
        width: u32,
        height: u32,
    ) -> Result<Self, ShmError> {
        let stride = width as usize * BYTES_PER_PIXEL;
        let len = stride * height as usize;
        let mapping = ShmMapping::alloc(len)?;

        let pool = shm.create_pool(mapping._fd.as_fd(), len as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );

        let busy = Arc::new(AtomicBool::new(false));
        // Register the busy flag in the shared release registry, keyed by
        // the wl_buffer proxy pointer. The Dispatch<wl_buffer> handler looks
        // up this key on `release` to clear the flag.
        let key = buffer.id().as_ptr() as usize;
        registry.lock().unwrap().insert(key, busy.clone());

        Ok(Self {
            mapping: ManuallyDrop::new(mapping),
            pool: ManuallyDrop::new(pool),
            buffer: ManuallyDrop::new(buffer),
            width,
            height,
            busy,
            registry: registry.clone(),
            key,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    /// True while the compositor still holds this buffer.
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// The writable pixel pointer (CPU side). Valid while the `ShmBuffer`
    /// lives; the compositor reads the same mmap pages.
    pub fn pixels(&self) -> *mut u8 {
        self.mapping.as_mut_ptr()
    }

    pub fn pixel_len(&self) -> usize {
        self.mapping.len
    }

    /// The `wl_buffer` to attach to the surface. Marks the buffer busy; the
    /// `release` event clears it. Caller is responsible for the
    /// `attach`/`damage`/`commit` sequence after.
    pub fn wl_buffer(&self) -> &wl_buffer::WlBuffer {
        &self.buffer
    }

    /// Mark this buffer as handed to the compositor. Called right before the
    /// host issues `wl_surface.attach`.
    pub fn mark_busy(&self) {
        self.busy.store(true, Ordering::Release);
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        // Deregister from the release registry first so the map cannot
        // accumulate stale entries across pool reallocations (each resize
        // creates a fresh `wl_buffer` with a new key; without this the old
        // key would leak forever). Removing the entry before destroying the
        // proxy is safe: wayland-client won't deliver `release` to a proxy
        // we're about to destroy, and the key is unique to this buffer.
        if let Ok(mut reg) = self.registry.lock() {
            reg.remove(&self.key);
        }
        // Drop order: buffer first (sends wl_buffer_destroy → compositor
        // releases its ref), then pool, then munmap. ManuallyDrop fields
        // are dropped explicitly; busy + registry (Arc) auto-drop after.
        unsafe {
            ManuallyDrop::drop(&mut self.buffer);
            ManuallyDrop::drop(&mut self.pool);
            ManuallyDrop::drop(&mut self.mapping);
        }
    }
}

/// A fixed-capacity double-buffered pool of `ShmBuffer`s for the panel.
///
/// `acquire()` returns a free buffer (one whose `busy` flag is false, and
/// whose size matches the current surface). If both are busy or the wrong
/// size, it returns `None` — the caller drops the frame and tries again next
/// tick. This is the non-blocking heart of the design: the panel can never
/// be held hostage by a compositor that doesn't recycle buffers.
pub struct ShmBufferPool {
    buffers: Vec<ShmBuffer>,
    cap: usize,
    shm: wl_shm::WlShm,
    qh: QueueHandle<InputMethodState>,
    registry: ShmReleaseRegistry,
}

impl ShmBufferPool {
    pub const DEFAULT_CAP: usize = 2;

    pub fn new(
        shm: wl_shm::WlShm,
        qh: QueueHandle<InputMethodState>,
        registry: ShmReleaseRegistry,
    ) -> Self {
        Self {
            buffers: Vec::new(),
            cap: Self::DEFAULT_CAP,
            shm,
            qh,
            registry,
        }
    }

    /// Find a free buffer matching `(width, height)`. Allocates a new one if
    /// under the cap and none is reusable. Returns `None` if all matching
    /// buffers are busy (drop this frame) or the cap is reached and a
    /// differently-sized buffer is the only one free (it gets reallocated
    /// in place).
    pub fn acquire(&mut self, width: u32, height: u32) -> Option<usize> {
        // 1. Try a free, correctly-sized buffer.
        for (i, b) in self.buffers.iter().enumerate() {
            if !b.busy() && b.width() == width && b.height() == height {
                return Some(i);
            }
        }
        // 2. Replace a free, wrong-sized buffer (in-place realloc).
        for (i, b) in self.buffers.iter_mut().enumerate() {
            if !b.busy() && (b.width() != width || b.height() != height) {
                match ShmBuffer::new(&self.shm, &self.qh, &self.registry, width, height) {
                    Ok(new) => {
                        self.buffers[i] = new;
                        return Some(i);
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "typio.panel.shm",
                            width, height, error = %e,
                            "shm buffer realloc failed"
                        );
                        return None;
                    }
                }
            }
        }
        // 3. Grow the pool up to cap.
        if self.buffers.len() < self.cap {
            match ShmBuffer::new(&self.shm, &self.qh, &self.registry, width, height) {
                Ok(new) => {
                    self.buffers.push(new);
                    return Some(self.buffers.len() - 1);
                }
                Err(e) => {
                    tracing::warn!(
                        target: "typio.panel.shm",
                        width, height, error = %e,
                        "shm buffer alloc failed"
                    );
                    return None;
                }
            }
        }
        // 4. All buffers busy — drop this frame.
        None
    }

    /// Borrow a buffer by index (from `acquire`).
    pub fn get(&self, idx: usize) -> Option<&ShmBuffer> {
        self.buffers.get(idx)
    }

    /// Borrow a buffer mutably by index.
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut ShmBuffer> {
        self.buffers.get_mut(idx)
    }

    /// True iff every buffer is busy (compositor holds them all).
    pub fn all_busy(&self) -> bool {
        !self.buffers.is_empty() && self.buffers.iter().all(|b| b.busy())
    }

    /// Current number of buffers.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Whether the pool has no buffers yet.
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

/// Errors from SHM buffer allocation.
#[derive(Debug)]
pub enum ShmError {
    OpenFd(std::io::Error),
    Fallocate(std::io::Error),
    Mmap(std::io::Error),
}

impl std::fmt::Display for ShmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShmError::OpenFd(e) => write!(f, "shm fd open failed: {e}"),
            ShmError::Fallocate(e) => write!(f, "shm fallocate failed: {e}"),
            ShmError::Mmap(e) => write!(f, "shm mmap failed: {e}"),
        }
    }
}

impl std::error::Error for ShmError {}

// ── Wayland dispatch for the buffer pool ──────────────────────────────────
//
// `wl_shm`, `wl_shm_pool`, and `wl_buffer` produce only the `release` event
// (on `wl_buffer`); the other two are global/object-manager objects with no
// client-visible events. We implement `Dispatch` so wayland-client can route
// the `release` event back to the matching `ShmBuffer`, clearing its busy
// flag.
//
// We can't get the `ShmBuffer` from the proxy directly, so we look up the
// buffer by its Wayland id inside the pool stored on `InputMethodState`. The
// `release` handler walks the pool and clears the matching buffer's busy
// flag.

impl Dispatch<wl_shm::WlShm, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        _event: <wl_shm::WlShm as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // wl_shm has only a `format` event we don't need (Argb8888 is
        // guaranteed by the spec).
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: <wl_shm_pool::WlShmPool as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // No events.
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for InputMethodState {
    fn event(
        state: &mut Self,
        proxy: &wl_buffer::WlBuffer,
        event: <wl_buffer::WlBuffer as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_buffer::Event;
        if let Event::Release = event {
            // Look up the busy flag in the shared release registry.
            let key = proxy.id().as_ptr() as usize;
            if let Some(busy) = state.shm_release_registry().lock().unwrap().get(&key) {
                busy.store(false, Ordering::Release);
            }
        }
    }
}
