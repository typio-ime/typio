//! Host-managed SHM buffer pool for the candidate panel.
//!
//! flux renders to a CPU canvas (`flux_canvas_cpu_*`), and this module hands
//! the frame to the compositor via a `wl_shm` `wl_buffer`. The host owns the
//! buffer lifecycle: `wl_buffer.release` is a normal event on the host's own
//! queue, so a compositor that stops recycling buffers can at worst cause
//! dropped frames — there is no blocking present call to stall on.
//!
//! Design mirrors fcitx5's `Buffer` / `WaylandShmWindow` (small fixed pool,
//! non-blocking `acquire`, `release`-driven reuse), translated to Rust +
//! wayland-client 0.31.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::{Dispatch, Proxy, QueueHandle};

use crate::input_method::InputMethodState;

/// ARGB8888: 4 bytes per pixel (Wayland's mandatory baseline format).
const BYTES_PER_PIXEL: usize = 4;

/// Per-buffer release state, passed as the `wl_buffer`'s own wayland
/// user-data at `create_buffer` time and handed back by the
/// `Dispatch<wl_buffer, BufferReleaseState>` `release` handler.
///
/// This replaces an earlier global `HashMap<proxy-pointer, Arc<AtomicBool>>`
/// registry. That design had two structural defects:
///
/// - the key was the raw `wl_proxy` address, which libwayland is free to
///   reuse for the next `wl_buffer` after destruction, so a late `release`
///   for a destroyed buffer could clear the *new* buffer's busy flag early;
/// - the registry itself was a second source of truth that had to be
///   manually kept in sync on every buffer replacement.
///
/// With per-buffer user-data the mapping is owned by the wayland-client
/// object itself: it is born with the buffer, travels with the exact proxy
/// that receives `release`, and dies with it. No cross-lookup, no ABA race,
/// no bookkeeping.
#[derive(Debug, Default)]
pub struct BufferReleaseState {
    busy: AtomicBool,
}

impl BufferReleaseState {
    fn clear_busy(&self) {
        self.busy.store(false, Ordering::Release);
    }
    fn mark_busy(&self) {
        self.busy.store(true, Ordering::Release);
    }
    fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

/// One SHM-backed `wl_buffer` with an mmap'd pixel region.
///
/// `busy` tracks whether the compositor still holds this buffer (it was
/// attached+committed and `wl_buffer.release` has not yet arrived). The flag
/// lives in the buffer's own wayland user-data (`BufferReleaseState`), so
/// the `Dispatch<wl_buffer, BufferReleaseState>` handler clears exactly the
/// right buffer with no cross-lookup.
pub struct ShmBuffer {
    mapping: ManuallyDrop<ShmMapping>,
    pool: ManuallyDrop<wl_shm_pool::WlShmPool>,
    buffer: ManuallyDrop<wl_buffer::WlBuffer>,
    width: u32,
    height: u32,
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
    let fd = unsafe {
        libc::syscall(
            libc::SYS_memfd_create,
            name.as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
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
    /// the `release` event. The buffer's busy flag rides inside the
    /// `wl_buffer`'s own user-data; there is no separate registry.
    pub fn new(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<InputMethodState>,
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
            BufferReleaseState::default(),
        );

        Ok(Self {
            mapping: ManuallyDrop::new(mapping),
            pool: ManuallyDrop::new(pool),
            buffer: ManuallyDrop::new(buffer),
            width,
            height,
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
        release_state(&self.buffer).is_busy()
    }

    /// The writable pixel pointer (CPU side). Valid while the `ShmBuffer`
    /// lives; the compositor reads the same mmap pages.
    pub fn pixels(&self) -> *mut u8 {
        self.mapping.as_mut_ptr()
    }

    pub fn pixel_len(&self) -> usize {
        self.mapping.len
    }

    /// The `wl_buffer` to attach to the surface. The caller marks it busy and
    /// performs the `attach`/`damage`/`commit` sequence immediately after.
    pub fn wl_buffer(&self) -> &wl_buffer::WlBuffer {
        &self.buffer
    }

    /// Mark this buffer as handed to the compositor. Called right before the
    /// host issues `wl_surface.attach`.
    pub fn mark_busy(&self) {
        release_state(&self.buffer).mark_busy();
    }
}

/// Read the busy-flag back out of a `wl_buffer`'s wayland user-data.
///
/// The user-data was installed at `create_buffer` time as a
/// `BufferReleaseState`; wayland-client stores it inside a `QueueProxyData`
/// wrapper and hands it to `Dispatch::event` as `&U`. `Proxy::data::<U>()`
/// is the public read side of the same slot. A `None` here would mean the
/// proxy was not created by us — unreachable for pool-owned buffers.
fn release_state(buffer: &wl_buffer::WlBuffer) -> &BufferReleaseState {
    buffer
        .data::<BufferReleaseState>()
        .expect("wl_buffer created without BufferReleaseState user-data")
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        // Drop order: buffer first (sends wl_buffer_destroy → compositor
        // releases its ref), then pool, then munmap. ManuallyDrop fields
        // are dropped explicitly. The release state rides inside the
        // wl_buffer's own user-data and is freed with the proxy — no
        // separate registry to deregister from.
        unsafe {
            ManuallyDrop::drop(&mut self.buffer);
            ManuallyDrop::drop(&mut self.pool);
            ManuallyDrop::drop(&mut self.mapping);
        }
    }
}

/// A fixed-capacity pool of `ShmBuffer`s for the panel.
///
/// `acquire()` returns a free buffer (one whose `busy` flag is false, and
/// whose size matches the current surface). If all are busy, the caller drops
/// the frame and tries again after a later release event. This is the
/// non-blocking heart of the design: the Panel never waits for the compositor.
pub struct ShmBufferPool {
    buffers: Vec<ShmBuffer>,
    cap: usize,
    shm: wl_shm::WlShm,
    qh: QueueHandle<InputMethodState>,
}

impl ShmBufferPool {
    // Triple buffering absorbs one compositor-release delay during rapid
    // candidate-highlight repeats without blocking the input path. The pool is
    // still tiny and non-blocking: if the compositor falls further behind,
    // drawing is skipped and the latest dirty state is retried on a later
    // reactor step.
    pub const DEFAULT_CAP: usize = 3;

    pub fn new(shm: wl_shm::WlShm, qh: QueueHandle<InputMethodState>) -> Self {
        Self {
            buffers: Vec::new(),
            cap: Self::DEFAULT_CAP,
            shm,
            qh,
        }
    }

    /// Find a free buffer matching `(width, height)`. Allocates a new one if
    /// under the cap and none is reusable.
    pub fn acquire(&mut self, width: u32, height: u32) -> Result<usize, ShmAcquireError> {
        // 1. Try a free, correctly-sized buffer.
        for (i, b) in self.buffers.iter().enumerate() {
            if !b.busy() && b.width() == width && b.height() == height {
                return Ok(i);
            }
        }
        // 2. Replace a free, wrong-sized buffer (in-place realloc).
        for (i, b) in self.buffers.iter_mut().enumerate() {
            if !b.busy() && (b.width() != width || b.height() != height) {
                match ShmBuffer::new(&self.shm, &self.qh, width, height) {
                    Ok(new) => {
                        self.buffers[i] = new;
                        return Ok(i);
                    }
                    Err(error) => return Err(ShmAcquireError::Allocation(error)),
                }
            }
        }
        // 3. Grow the pool up to cap.
        if self.buffers.len() < self.cap {
            match ShmBuffer::new(&self.shm, &self.qh, width, height) {
                Ok(new) => {
                    self.buffers.push(new);
                    return Ok(self.buffers.len() - 1);
                }
                Err(error) => return Err(ShmAcquireError::Allocation(error)),
            }
        }
        // 4. All buffers busy — drop this frame.
        Err(ShmAcquireError::Busy)
    }

    /// Borrow a buffer by index (from `acquire`).
    pub fn get(&self, idx: usize) -> Option<&ShmBuffer> {
        self.buffers.get(idx)
    }

    /// True iff every buffer is busy (compositor holds them all).
    pub fn all_busy(&self) -> bool {
        !self.buffers.is_empty() && self.buffers.iter().all(|b| b.busy())
    }

    /// Current number of buffers.
    pub(crate) fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Drop every cached buffer and start fresh on the next acquire.
    ///
    /// Used after system resume: some compositors can lose or indefinitely delay
    /// `wl_buffer.release` for popup buffers that were in-flight across suspend,
    /// leaving the tiny non-blocking pool permanently busy. Reallocating fresh
    /// buffers prevents candidate-highlight frames from being dropped forever.
    pub fn reset(&mut self) {
        self.buffers.clear();
    }
}

/// Why a non-blocking SHM buffer reservation failed.
#[derive(Debug)]
pub enum ShmAcquireError {
    /// The compositor still owns every buffer in the fixed-capacity pool.
    Busy,
    /// Allocating or resizing a free buffer failed.
    Allocation(ShmError),
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
// client-visible events. The buffer's busy flag travels inside the
// `wl_buffer`'s own wayland user-data (`BufferReleaseState`, installed at
// `create_buffer` time), so the `release` handler receives the exact state
// of the exact buffer as its `data` argument — no lookup, no proxy-pointer
// keying, no possibility of clearing a different buffer's flag.

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

impl Dispatch<wl_buffer::WlBuffer, BufferReleaseState> for InputMethodState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        event: <wl_buffer::WlBuffer as wayland_client::Proxy>::Event,
        data: &BufferReleaseState,
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_buffer::Event;
        if let Event::Release = event {
            data.clear_busy();
        }
    }
}
