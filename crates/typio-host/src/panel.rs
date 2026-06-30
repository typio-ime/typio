//! Candidate panel — flux offscreen rendering + host-managed SHM buffer.
//!
//! flux renders to an **offscreen** Vulkan image (no swapchain, no WSI), the
//! host reads the pixels back via `flux_surface_read_pixels`, and attaches
//! them to the `wl_surface` through a `wl_shm` `wl_buffer` the host owns.
//! This removes `vkQueuePresentKHR` from the panel's critical path entirely:
//! the WSI present call that could block the main thread for 16 s while
//! Mesa's Wayland WSI waited for the compositor to recycle swapchain images
//! is never invoked. A compositor that stops recycling buffers now only
//! causes dropped frames — the host's `wl_buffer.release` handler is a
//! normal event-loop event, so the main thread keeps pumping.
//!
//! ## Architecture
//!
//! ```text
//! Vulkan device (no WSI extensions)
//!   → flux_surface (offscreen: vk_surface_khr = NULL, owns RGBA8 images)
//!     → flux_canvas → flux_text_draw  (GPU render, unchanged)
//!       → flux_frame_submit
//!         → flux_surface_read_pixels  (GPU→CPU, bounded by fence timeout)
//!           → host: memcpy into a free ShmBuffer
//!             → wl_surface.attach + damage + commit  (raw FFI, returns instantly)
//!               → compositor composites; wl_buffer.release reuses the slot
//! ```

use std::ffi::c_void;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;
use std::time::{Duration, Instant};

use flux_sys::{
    flux_arena, flux_arena_destroy, flux_arena_init, flux_arena_reset, flux_canvas,
    flux_canvas_begin, flux_canvas_desc, flux_canvas_destroy, flux_canvas_end, flux_color_rgba,
    flux_device, flux_device_create, flux_device_desc, flux_device_release, flux_error_info,
    flux_frame, flux_frame_begin_desc, flux_frame_present, flux_frame_submit, flux_get_last_error,
    flux_struct_type, flux_surface, flux_surface_begin_frame, flux_surface_create,
    flux_surface_desc, flux_surface_read_pixels, flux_surface_release,
};
use flux_text_sys::{
    flux_text, flux_text_create, flux_text_desc, flux_text_destroy, flux_text_draw,
    flux_text_family, flux_text_metrics,
};
use wayland_client::protocol::wl_shm;
use wayland_client::{Proxy, QueueHandle};
use wayland_sys::{
    client::{wl_proxy, wl_proxy_marshal_array},
    common::wl_argument,
};

use crate::protocols::viewporter::wp_viewport::WpViewport;

use flux_struct_type as FType;
use flux_text_family as FontFamily;

/// Frame-begin timeout for offscreen panel rendering (200 ms). Short enough
/// that a stuck GPU frame setup fails fast instead of blocking the main loop
/// past the watchdog's 3-second stuck threshold.
const PANEL_FRAME_TIMEOUT_NS: u64 = 200_000_000;
/// Offscreen image width quantum (ADR-0013, adapted by ADR-0040). The buffer is allocated in
/// multiples of this and grows only; sub-quantum widenings reuse the
/// existing offscreen image and are cropped to the exact content rect with
/// `wp_viewport`. 64 px is large enough that typical candidate-row
/// width variation stays inside one quantum, so after a short warm-up
/// `flux_surface_resize` is never called again during steady-state paging.
const SURFACE_WIDTH_QUANTUM: u32 = 64;
/// Height quantum (same grow-only logic as width). Banner and
/// candidate rows are both ~40 px at scale 1; a 32 px quantum rounds
/// the first request up to 64 px, fitting inside the pre-allocation
/// in `InputMethodFrontend::connect` and so avoiding a first-render
/// `flux_surface_resize` on the very first indicator banner. Without this,
/// height was exact-matched while
/// width was grow-only — a quiet asymmetry that made every panel
/// flush in a new process pay a resize.
const SURFACE_HEIGHT_QUANTUM: u32 = 32;
/// Initial offscreen image size passed to `FluxPanel::new_from_surface` by
/// `InputMethodFrontend::connect`. Sized to skip the first automatic
/// indicator banner's `flux_surface_resize` at the common display scales.
///
/// Banner geometry for the longest observed default indicator label
/// `"中 · Rime · 懿拼音"` (text metric 119.3 px logical, plus
/// `2 * BANNER_PADDING = 20`):
///
/// | scale | phys (W×H) | quantised (W×H) |
/// |------:|-----------|-----------------|
/// |  1.0  | 140 × 40  | 192 × 64        |
/// |  1.5  | 210 × 60  | 256 × 64        |
/// |  2.0  | 280 × 80  | 320 × 96        |
/// |  3.0  | 420 × 120 | 448 × 128       |
///
/// `512 × 128` covers every cell in the table with one width-quantum
/// of headroom (512 − 448 = 64). Larger indicator labels (long engine
/// names, verbose mode displays) and scales ≥ 4 still trigger a
/// one-time resize — but only after the user has actually started
/// typing or switched engine, by which point the watchdog tolerance
/// has been replaced by genuine interaction cadence.
///
/// `PANEL_PREALLOC_WIDTH` is a multiple of `SURFACE_WIDTH_QUANTUM`,
/// `PANEL_PREALLOC_HEIGHT` of `SURFACE_HEIGHT_QUANTUM`; this keeps the
/// initial allocation on a quantum boundary so the first grow-only
/// decision in `apply_grow_only_size` is a no-op when the content fits.
pub const PANEL_PREALLOC_WIDTH: u32 = 512;
pub const PANEL_PREALLOC_HEIGHT: u32 = 128;
const PANEL_PADDING: f32 = 8.0;
const PANEL_ROW_HEIGHT: f32 = 24.0;
const CANDIDATE_FONT_SIZE: f32 = 16.0;
const CANDIDATE_ITEM_X_PADDING: f32 = 5.0;
const CANDIDATE_ITEM_GAP: f32 = 8.0;
const CANDIDATE_NUMBER_FONT_SIZE: f32 = 11.0;
const CANDIDATE_NUMBER_GAP: f32 = 4.0;
/// Status-banner (indicator / voice) layout constants. Kept separate from
/// the candidate-row metrics above: the banner is a single centred text
/// segment, not a two-column "number + candidate" row, so its padding and
/// font size are tuned independently. The same offscreen surface / flux_text /
/// atlas stack is shared (ADR-0017 — one popup surface, mutually exclusive
/// owners).
const BANNER_PADDING: f32 = 10.0;
const BANNER_FONT_SIZE: f32 = 15.0;
/// Computed banner row height: padding above + font box + padding below.
/// `1.3` is the typical line-height factor flux applies for default fonts.
const BANNER_ROW_HEIGHT: f32 = BANNER_PADDING * 2.0 + BANNER_FONT_SIZE * 1.3;

const PANEL_TIMING_TARGET: &str = "typio.panel.timing";
const PANEL_PROBE_TARGET: &str = "typio.panel.probe";
const PANEL_TIMING_SLOW_THRESHOLD: Duration = Duration::from_millis(12);

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[derive(Clone, Copy, Debug, Default)]
struct TextStatsSnapshot {
    glyph_count: u64,
    glyph_cap: u64,
    glyph_hits: u64,
    glyph_misses: u64,
    glyph_evictions: u64,
    atlas_clears: u64,
}

unsafe fn text_stats_snapshot(text: *mut flux_text) -> TextStatsSnapshot {
    let mut stats: flux_text_sys::flux_text_stats = unsafe { std::mem::zeroed() };
    unsafe {
        flux_text_sys::flux_text_get_stats(text, &mut stats);
    }
    TextStatsSnapshot {
        glyph_count: stats.glyph_count as u64,
        glyph_cap: stats.glyph_cap as u64,
        glyph_hits: stats.glyph_hits,
        glyph_misses: stats.glyph_misses,
        glyph_evictions: stats.glyph_evictions,
        atlas_clears: stats.atlas_clears,
    }
}

/// Whether the panel probe target is enabled through `tracing`.
///
/// The probe is high-volume enough that it should not collect stats unless
/// `RUST_LOG=typio.panel.probe=debug` (or a wider debug/trace floor) enables
/// it.
fn panel_probe_enabled() -> bool {
    tracing::enabled!(target: PANEL_PROBE_TARGET, tracing::Level::DEBUG)
}

/// Compact tracing probe for diagnosing "candidate switching lags after a
/// while". Accumulates per-frame present timing and glyph-cache churn,
/// then emits one summary event every `PROBE_WINDOW` frames — plus an
/// immediate event whenever `atlas_clears` rises (the canonical
/// atlas-thrash signal) so a saturating atlas is caught the moment it
/// starts, not `PROBE_WINDOW` frames later.
///
/// The three numbers to watch over a long session:
///   * `atlas_clears` climbing       → glyph atlas thrash (see atlas.c)
///   * `evict/win` consistently > 0  → glyph cache over its working set
///   * `present_max_ms` climbing     → GPU submit/readback or SHM attach cost
fn emit_panel_probe(
    present: Duration,
    total: Duration,
    candidate_count: usize,
    stats: &TextStatsSnapshot,
) {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Frames per aggregated tracing event.
    const PROBE_WINDOW: u64 = 120;

    static FRAMES: AtomicU64 = AtomicU64::new(0);
    static PRESENT_MAX_US: AtomicU64 = AtomicU64::new(0);
    static TOTAL_MAX_US: AtomicU64 = AtomicU64::new(0);
    static SLOW_FRAMES: AtomicU64 = AtomicU64::new(0);
    static LAST_CLEARS: AtomicU64 = AtomicU64::new(u64::MAX);
    static LAST_EVICTIONS: AtomicU64 = AtomicU64::new(0);

    let present_us = present.as_micros() as u64;
    let total_us = total.as_micros() as u64;
    PRESENT_MAX_US.fetch_max(present_us, Ordering::Relaxed);
    TOTAL_MAX_US.fetch_max(total_us, Ordering::Relaxed);
    if total >= PANEL_TIMING_SLOW_THRESHOLD {
        SLOW_FRAMES.fetch_add(1, Ordering::Relaxed);
    }
    let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;

    // Immediate alert on atlas-clear: the single most diagnostic event
    // for the "lags after a while" regression. u64::MAX sentinel skips
    // the first frame so we don't false-alarm on the initial read.
    let last_clears = LAST_CLEARS.swap(stats.atlas_clears, Ordering::Relaxed);
    if last_clears != u64::MAX && stats.atlas_clears > last_clears {
        tracing::debug!(
            target: PANEL_PROBE_TARGET,
            atlas_clears = stats.atlas_clears,
            glyph_count = stats.glyph_count,
            glyph_cap = stats.glyph_cap,
            "panel probe atlas clear"
        );
    }

    if n % PROBE_WINDOW == 0 {
        let present_max_ms = PRESENT_MAX_US.swap(0, Ordering::Relaxed) as f64 / 1000.0;
        let total_max_ms = TOTAL_MAX_US.swap(0, Ordering::Relaxed) as f64 / 1000.0;
        let slow = SLOW_FRAMES.swap(0, Ordering::Relaxed);
        let evict_delta = stats
            .glyph_evictions
            .saturating_sub(LAST_EVICTIONS.swap(stats.glyph_evictions, Ordering::Relaxed));
        tracing::debug!(
            target: PANEL_PROBE_TARGET,
            frames = n,
            window = PROBE_WINDOW,
            candidate_count,
            present_max_ms,
            total_max_ms,
            slow_frames = slow,
            glyph_count = stats.glyph_count,
            glyph_cap = stats.glyph_cap,
            atlas_clears = stats.atlas_clears,
            glyph_evictions_delta = evict_delta,
            "panel probe window"
        );
    }
}

// ── Pipeline-cache persistence ────────────────────────────────────────
//
// flux's new pipeline-cache API (Skia PersistentCache model) lets the
// consumer own the storage strategy via load/save callbacks on
// flux_device_desc. We persist to $XDG_CACHE_HOME/typio/pipeline.bin
// (or $HOME/.cache/typio/pipeline.bin) so shader compilation cost is
// paid once; subsequent daemon starts reuse the cached VkPipelineCache
// blob. The load callback returns a libc::malloc'd buffer (flux frees
// it with C free()); the save callback writes atomically (temp +
// rename). Both are best-effort and silent on failure — the cache is
// an optimisation, not a correctness path.

/// Resolve the pipeline-cache path following XDG conventions.
/// Returns None when no cache home is available.
fn pipeline_cache_path() -> Option<std::path::PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(std::path::PathBuf::from(xdg).join("typio/pipeline.bin"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            return Some(std::path::PathBuf::from(home).join(".cache/typio/pipeline.bin"));
        }
    }
    None
}

unsafe extern "C" fn pipeline_cache_load(
    userdata: *mut c_void,
    out_size: *mut usize,
) -> *mut c_void {
    if userdata.is_null() || out_size.is_null() {
        return ptr::null_mut();
    }
    let path = std::ffi::CStr::from_ptr(userdata as *const c_char);
    let path = match path.to_str() {
        Ok(s) => std::path::Path::new(s),
        Err(_) => return ptr::null_mut(),
    };
    match std::fs::read(path) {
        Ok(data) => {
            let size = data.len();
            let buf = libc::malloc(size);
            if buf.is_null() {
                return ptr::null_mut();
            }
            ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, size);
            *out_size = size;
            buf
        }
        Err(_) => {
            *out_size = 0;
            ptr::null_mut()
        }
    }
}

unsafe extern "C" fn pipeline_cache_save(userdata: *mut c_void, data: *const c_void, size: usize) {
    if userdata.is_null() || data.is_null() || size == 0 {
        return;
    }
    let path = std::ffi::CStr::from_ptr(userdata as *const c_char);
    let path = match path.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => return,
    };
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let bytes = std::slice::from_raw_parts(data as *const u8, size);
    let tmp = format!("{path}.tmp.{pid}", pid = std::process::id());
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn free_cache_path(p: *mut c_char) {
    if !p.is_null() {
        unsafe {
            let _ = std::ffi::CString::from_raw(p);
        }
    }
}

/// A candidate panel backed by flux offscreen rendering and Wayland SHM
/// presentation.
///
/// The panel renders candidates through flux into an offscreen image, reads
/// the pixels back, and attaches them to the input-method popup `wl_surface`
/// via host-managed `wl_shm` buffers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelStyle {
    Classic,
    LiquidGlass,
}

impl Default for PanelStyle {
    fn default() -> Self {
        Self::Classic
    }
}

pub struct FluxPanel {
    pub style: PanelStyle,
    device: *mut flux_device,
    surface: *mut flux_surface,
    canvas: *mut flux_canvas,
    text: *mut flux_text,
    arena: flux_arena,
    width: u32,
    height: u32,
    scale: f32,
    /// `wp_viewport` on the panel surface, used to crop the grow-only
    /// offscreen image to the exact content rect (ADR-0013, adapted to the
    /// offscreen+shm path). `None` only when the compositor lacks
    /// `wp_viewporter`; in that case the offscreen image is sized exactly
    /// to the content and the per-page resize cost returns.
    viewport: Option<WpViewport>,
    /// Last source size (physical buffer pixels) sent to `wp_viewport`.
    /// This must be tracked separately from the logical destination: moving
    /// the popup from a 1x output to a 2x output can leave the logical panel
    /// size unchanged while doubling the source rectangle.
    viewport_source_w_physical: u32,
    viewport_source_h_physical: u32,
    /// Last destination size (logical, pre-scale) sent to `wp_viewport`.
    /// Tracked so we only re-issue set_source/set_destination when the
    /// visible mapping actually changes, not on every redraw.
    content_w_logical: i32,
    content_h_logical: i32,
    last_candidate_size_duration: Duration,
    last_candidate_size_resized: bool,
    // Keep the raw wl_surface alive for the panel's lifetime; the host
    // attaches shm buffers to it directly (no VkSurfaceKHR anymore).
    wl_surface: *mut c_void,
    /// Host-managed SHM buffer pool (double-buffered). Replaces the WSI
    /// swapchain: `acquire` returns a free buffer or `None` (drop frame),
    /// `wl_buffer.release` reuses a slot. `None` when the compositor lacks
    /// `wl_shm`.
    shm_pool: Option<crate::panel_shm::ShmBufferPool>,
    /// dma-buf buffer pool for zero-copy present (ADR-0040 follow-on). Used
    /// when the compositor supports `zwp_linux_dmabuf_v1` and flux exported
    /// the offscreen image. Falls back to `shm_pool` + readback otherwise.
    dmabuf_pool: Option<crate::panel_dmabuf::DmabufBufferPool>,
    /// Heap-allocated C string for the pipeline-cache path. Owned by
    /// FluxPanel so it outlives `flux_device_release` (which fires the
    /// save callback). Null when cache persistence is unavailable
    /// (no XDG_CACHE_HOME / HOME).
    pipeline_cache_path: *mut c_char,
    /// Cached per-candidate layout: `(number_metrics, text_metrics)`
    /// for each entry in `last_layout_key.0`, in logical pixels.
    /// Recomputed only when the candidate strings or `scale` change.
    /// Both `ensure_candidate_size` (total-width sizing) and
    /// `draw_candidates` (per-item placement) consult this cache, so a
    /// candidate-highlight-only update — the canonical Up/Down arrow
    /// navigation case — skips the `flux_text_measure` FFI loop
    /// entirely.
    last_layout_key: Option<(Vec<String>, f32)>,
    last_layout: Vec<(flux_text_metrics, flux_text_metrics)>,
}

impl FluxPanel {
    /// Create a panel that renders offscreen via flux and presents through a
    /// host-managed `wl_shm` buffer on `wl_surface`.
    ///
    /// `wl_surface_ptr` must be a valid `*mut wl_surface` from the same
    /// Wayland connection the input-method frontend uses. It must outlive the
    /// panel.
    ///
    /// `shm` is the bound `wl_shm` global; `qh` is the event queue that will
    /// receive `wl_buffer.release` events for the panel's buffers.
    ///
    /// `viewport`, when present, attaches a `wp_viewport` to the surface so
    /// the offscreen image can be allocated grow-only and cropped to exact
    /// content (ADR-0013). `None` falls back to exact-size reallocation.
    ///
    /// # Safety
    /// `wl_surface_ptr` must be valid for the panel's lifetime.
    pub unsafe fn new_from_surface(
        wl_surface_ptr: *mut c_void,
        viewport: Option<WpViewport>,
        shm: Option<wl_shm::WlShm>,
        dmabuf: Option<crate::protocols::linux_dmabuf_v1::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
        qh: QueueHandle<crate::input_method::InputMethodState>,
        registry: crate::panel_shm::ShmReleaseRegistry,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        Self::new_inner(wl_surface_ptr, viewport, shm, dmabuf, qh, registry, width, height)
    }

    fn new_inner(
        wl_surface_ptr: *mut c_void,
        viewport: Option<WpViewport>,
        shm: Option<wl_shm::WlShm>,
        dmabuf: Option<crate::protocols::linux_dmabuf_v1::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
        qh: QueueHandle<crate::input_method::InputMethodState>,
        registry: crate::panel_shm::ShmReleaseRegistry,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        unsafe {
            Self::new_inner_unsafe(wl_surface_ptr, viewport, shm, dmabuf, qh, registry, width, height)
        }
    }

    unsafe fn new_inner_unsafe(
        wl_surface_ptr: *mut c_void,
        viewport: Option<WpViewport>,
        shm: Option<wl_shm::WlShm>,
        dmabuf: Option<crate::protocols::linux_dmabuf_v1::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
        qh: QueueHandle<crate::input_method::InputMethodState>,
        registry: crate::panel_shm::ShmReleaseRegistry,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        // 1. Use the provided wl_surface directly.
        let wl_surface = wl_surface_ptr;
        if wl_surface.is_null() {
            return Err("wl_surface is null".into());
        }

        // 2. Create Vulkan device WITHOUT WSI extensions. The offscreen
        //    surface needs no swapchain, no VkSurfaceKHR, no
        //    VK_KHR_surface / VK_KHR_wayland_surface / VK_KHR_swapchain.
        //    Dropping them also drops Mesa's wl_display-dispatching present
        //    path — the source of the 16 s present deadlock.
        //
        //    When the compositor supports linux-dmabuf, we additionally
        //    enable the external-memory / DRM-modifier extensions so the
        //    offscreen image can be exported as a dma-buf (zero-copy present,
        //    replacing the 12–16 ms GPU→CPU readback). The instance-side
        //    capability extensions (`VK_KHR_external_memory_capabilities`,
        //    `VK_KHR_get_physical_device_properties2`) must be requested at
        //    instance level; the rest are device extensions.
        let dmabuf_capable = dmabuf.is_some();
        let instance_ext_list: Vec<&'static CStr> = if dmabuf_capable {
            vec![c"VK_KHR_external_memory_capabilities",
                 c"VK_KHR_get_physical_device_properties2"]
        } else {
            vec![]
        };
        let device_ext_list: Vec<&'static CStr> = if dmabuf_capable {
            vec![c"VK_KHR_external_memory_fd",
                 c"VK_EXT_external_memory_dma_buf",
                 c"VK_EXT_image_drm_format_modifier",
                 c"VK_EXT_queue_family_foreign"]
        } else {
            vec![]
        };
        let instance_exts: Vec<*const c_char> =
            instance_ext_list.iter().map(|s| s.as_ptr()).collect();
        let device_exts: Vec<*const c_char> =
            device_ext_list.iter().map(|s| s.as_ptr()).collect();
        // Resolve pipeline-cache path before creating the desc so the
        // load/save callbacks and userdata can be wired in.
        let cache_path_c: *mut c_char = match &pipeline_cache_path() {
            Some(path) => {
                let cstring = std::ffi::CString::new(path.to_string_lossy().as_ref())
                    .map_err(|e| format!("cache path has NUL: {e}"))?;
                cstring.into_raw()
            }
            None => ptr::null_mut(),
        };
        let mut device_desc: flux_device_desc = std::mem::zeroed();
        device_desc.type_ = FType::FLUX_TYPE_DEVICE_DESC;
        device_desc.required_instance_extensions = instance_exts.as_ptr();
        device_desc.required_instance_extension_count = instance_exts.len() as u32;
        device_desc.required_device_extensions = device_exts.as_ptr();
        device_desc.required_device_extension_count = device_exts.len() as u32;
        device_desc.frames_in_flight = 2;
        if !cache_path_c.is_null() {
            device_desc.pipeline_cache_load = Some(pipeline_cache_load);
            device_desc.pipeline_cache_save = Some(pipeline_cache_save);
            device_desc.pipeline_cache_userdata = cache_path_c as *mut c_void;
        }

        let mut device: *mut flux_device = ptr::null_mut();
        let r = flux_device_create(&device_desc, &mut device);
        if !flux_result_is_ok(r) {
            free_cache_path(cache_path_c);
            return Err(flux_last_error_string("flux_device_create"));
        }

        // 3. Create an OFFSCREEN flux surface (no VkSurfaceKHR, no swapchain,
        //    no WSI). flux owns RGBA8 images at width×height; the frame loop
        //    is unchanged (begin → draw → submit → present is a no-op for
        //    offscreen); flux_surface_read_pixels reads the result back.
        //    This is the structural fix for the present deadlock: there is no
        //    vkQueuePresentKHR in this path.
        let mut surface_desc: flux_surface_desc = std::mem::zeroed();
        surface_desc.type_ = FType::FLUX_TYPE_SURFACE_DESC;
        surface_desc.vk_surface_khr = ptr::null_mut(); // NULL → offscreen (ADR-0013)
        surface_desc.width = width;
        surface_desc.height = height;

        let mut surface: *mut flux_surface = ptr::null_mut();
        let r = flux_surface_create(device, &surface_desc, &mut surface);
        if !flux_result_is_ok(r) {
            flux_device_release(device);
            free_cache_path(cache_path_c);
            return Err(flux_last_error_string("flux_surface_create"));
        }

        // 5. Create flux canvas.
        let mut canvas_desc: flux_canvas_desc = std::mem::zeroed();
        canvas_desc.type_ = FType::FLUX_TYPE_CANVAS_DESC;
        canvas_desc.surface = surface;
        canvas_desc.scale = 1.0;

        let mut canvas: *mut flux_canvas = ptr::null_mut();
        let r = flux_sys::flux_canvas_create(&canvas_desc, &mut canvas);
        if !flux_result_is_ok(r) {
            flux_surface_release(surface);
            flux_device_release(device);
            free_cache_path(cache_path_c);
            return Err(flux_last_error_string("flux_canvas_create"));
        }

        // 6. Create flux text context.
        let mut text_desc: flux_text_desc = std::mem::zeroed();
        text_desc.device = device as *mut flux_text_sys::flux_device;
        text_desc.scale = 1.0;

        let mut text: *mut flux_text = ptr::null_mut();
        let r = flux_text_create(&text_desc, &mut text);
        if !flux_result_is_ok(r) {
            flux_sys::flux_canvas_destroy(canvas);
            flux_surface_release(surface);
            flux_device_release(device);
            free_cache_path(cache_path_c);
            return Err(flux_last_error_string("flux_text_create"));
        }

        // 7. Create arena for per-frame text shaping allocations.
        let mut arena: flux_arena = unsafe { std::mem::zeroed() };
        let r = flux_arena_init(&mut arena, 256 * 1024, ptr::null_mut());
        if !flux_result_is_ok(r) {
            flux_text_destroy(text);
            flux_sys::flux_canvas_destroy(canvas);
            flux_surface_release(surface);
            flux_device_release(device);
            free_cache_path(cache_path_c);
            return Err(flux_last_error_string("flux_arena_init"));
        }

        Ok(Self {
            device,
            surface,
            canvas,
            text,
            arena,
            width,
            height,
            scale: 1.0,
            viewport,
            viewport_source_w_physical: 0,
            viewport_source_h_physical: 0,
            content_w_logical: 0,
            content_h_logical: 0,
            last_candidate_size_duration: Duration::ZERO,
            last_candidate_size_resized: false,
            wl_surface,
            shm_pool: shm
                .as_ref()
                .map(|s| crate::panel_shm::ShmBufferPool::new(s.clone(), qh.clone(), registry.clone())),
            dmabuf_pool: dmabuf
                .as_ref()
                .map(|d| crate::panel_dmabuf::DmabufBufferPool::new(d.clone(), qh.clone(), registry)),
            pipeline_cache_path: cache_path_c,
            last_layout_key: None,
            last_layout: Vec::new(),
            style: PanelStyle::default(),
        })
    }

    /// Set the HiDPI scale factor for rendering.

    unsafe fn draw_panel_background(&mut self) {
        let glass_rect = flux_sys::flux_rect {
            x: 1.0,
            y: 1.0,
            w: self.content_w_logical as f32 - 2.0,
            h: self.content_h_logical as f32 - 2.0,
        };
        let glass_radius = 8.0;

        match self.style {
            PanelStyle::Classic => {
                let bg = flux_sys::flux_color_rgba(28, 28, 32, 255);
                flux_sys::flux_canvas_fill_rrect(self.canvas, glass_rect, glass_radius, bg);
            }
            PanelStyle::LiquidGlass => {
                let mut shape: *mut flux_sys::flux_path = std::ptr::null_mut();
                flux_sys::flux_path_create(&mut shape, &mut self.arena);
                if !shape.is_null() {
                    flux_sys::flux_path_add_round_rect(shape, glass_rect, glass_radius);

                    // 1. Frosted Acrylic base (Milky translucent)
                    // Since we can't do true Wayland background blur here, we increase the
                    // base opacity to simulate the diffusion of a frosted acrylic plate.
                    let mut vol_stops = [
                        flux_sys::flux_gradient_stop {
                            t: 0.00,
                            color: flux_sys::flux_color_rgba_premul(255, 255, 255, 100),
                        },
                        flux_sys::flux_gradient_stop {
                            t: 1.00,
                            color: flux_sys::flux_color_rgba_premul(220, 230, 240, 40),
                        },
                    ];
                    let mut vol = flux_sys::flux_paint_linear_gradient(
                        flux_sys::flux_point {
                            x: glass_rect.x,
                            y: glass_rect.y,
                        },
                        flux_sys::flux_point {
                            x: glass_rect.x,
                            y: glass_rect.y + glass_rect.h,
                        },
                        vol_stops.as_mut_ptr(),
                        2,
                    );
                    flux_sys::flux_canvas_fill_path(self.canvas, shape, &mut vol);

                    // 2. Inner refraction bevel (Thickness)
                    // Gives the glass a solid chunk feeling using a thick, soft inner stroke.
                    let mut bevel_shape: *mut flux_sys::flux_path = std::ptr::null_mut();
                    flux_sys::flux_path_create(&mut bevel_shape, &mut self.arena);
                    if !bevel_shape.is_null() {
                        let width = 3.0;
                        let inset = width / 2.0;
                        let mut inner_rect = glass_rect;
                        inner_rect.x += inset;
                        inner_rect.y += inset;
                        inner_rect.w -= width;
                        inner_rect.h -= width;
                        flux_sys::flux_path_add_round_rect(
                            bevel_shape,
                            inner_rect,
                            glass_radius - inset,
                        );

                        let mut bevel_stops = [
                            flux_sys::flux_gradient_stop {
                                t: 0.00,
                                color: flux_sys::flux_color_rgba_premul(255, 255, 255, 120),
                            },
                            flux_sys::flux_gradient_stop {
                                t: 0.50,
                                color: flux_sys::flux_color_rgba_premul(255, 255, 255, 0),
                            },
                            flux_sys::flux_gradient_stop {
                                t: 0.80,
                                color: flux_sys::flux_color_rgba_premul(0, 0, 0, 0),
                            },
                            flux_sys::flux_gradient_stop {
                                t: 1.00,
                                color: flux_sys::flux_color_rgba_premul(0, 0, 0, 50),
                            },
                        ];
                        let mut sp = flux_sys::flux_paint_linear_gradient(
                            flux_sys::flux_point {
                                x: glass_rect.x,
                                y: glass_rect.y,
                            },
                            flux_sys::flux_point {
                                x: glass_rect.x + glass_rect.w,
                                y: glass_rect.y + glass_rect.h,
                            },
                            bevel_stops.as_mut_ptr(),
                            4,
                        );
                        sp.stroke_width = width;
                        sp.join = flux_sys::flux_line_join::FLUX_JOIN_ROUND;
                        flux_sys::flux_canvas_stroke_path(self.canvas, bevel_shape, &sp);
                    }

                    // 3. Specular Sheen (Sharp top gloss)
                    let mut sheen_stops = [
                        flux_sys::flux_gradient_stop {
                            t: 0.00,
                            color: flux_sys::flux_color_rgba_premul(255, 255, 255, 100),
                        },
                        flux_sys::flux_gradient_stop {
                            t: 0.35,
                            color: flux_sys::flux_color_rgba_premul(255, 255, 255, 0),
                        },
                        flux_sys::flux_gradient_stop {
                            t: 1.00,
                            color: flux_sys::flux_color_rgba_premul(0, 0, 0, 0),
                        },
                    ];
                    let mut sheen = flux_sys::flux_paint_linear_gradient(
                        flux_sys::flux_point {
                            x: glass_rect.x,
                            y: glass_rect.y,
                        },
                        flux_sys::flux_point {
                            x: glass_rect.x,
                            y: glass_rect.y + glass_rect.h * 0.8,
                        },
                        sheen_stops.as_mut_ptr(),
                        3,
                    );
                    flux_sys::flux_canvas_fill_path(self.canvas, shape, &mut sheen);

                    // 4. Sharp Hairline Edge (Surface tension)
                    let mut hair_stops = [
                        flux_sys::flux_gradient_stop {
                            t: 0.00,
                            color: flux_sys::flux_color_rgba_premul(255, 255, 255, 220),
                        },
                        flux_sys::flux_gradient_stop {
                            t: 0.40,
                            color: flux_sys::flux_color_rgba_premul(255, 255, 255, 40),
                        },
                        flux_sys::flux_gradient_stop {
                            t: 1.00,
                            color: flux_sys::flux_color_rgba_premul(255, 255, 255, 80),
                        },
                    ];
                    let mut sp_hair = flux_sys::flux_paint_linear_gradient(
                        flux_sys::flux_point {
                            x: glass_rect.x,
                            y: glass_rect.y,
                        },
                        flux_sys::flux_point {
                            x: glass_rect.x + glass_rect.w,
                            y: glass_rect.y + glass_rect.h,
                        },
                        hair_stops.as_mut_ptr(),
                        3,
                    );
                    sp_hair.stroke_width = 1.0;
                    sp_hair.join = flux_sys::flux_line_join::FLUX_JOIN_ROUND;
                    flux_sys::flux_canvas_stroke_path(self.canvas, shape, &sp_hair);
                }
            }
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() < 0.01 {
            return;
        }
        self.scale = scale;
        unsafe {
            flux_sys::flux_canvas_set_scale(self.canvas, scale);
            flux_text_sys::flux_text_set_scale(self.text, scale);
        }
    }

    /// Drop the cached per-candidate text layout, forcing the next
    /// [`Self::draw_candidates`] / [`Self::ensure_candidate_size`] to re-run
    /// `flux_text_measure`.
    ///
    /// The layout cache key ([`Self::last_layout_key`]) covers only the
    /// candidate strings and the rendering scale — the inputs that move today.
    /// It deliberately does **not** include font size/family/weight, which are
    /// currently compile-time constants ([`CANDIDATE_FONT_SIZE`],
    /// [`FontFamily::FLUX_TEXT_FAMILY_DEFAULT`]). Any path that can change a
    /// font/style/theme input the measure step depends on must call this so the
    /// cache cannot serve stale geometry. It is the layout-side counterpart of
    /// `PanelPresentationGuard`'s presentation invalidation.
    pub fn invalidate_layout_cache(&mut self) {
        self.last_layout_key = None;
        self.last_layout.clear();
    }

    /// Draw candidate strings with the selected one highlighted.
    ///
    /// `heartbeat` is invoked between each potentially blocking FFI call
    /// (`flux_surface_begin_frame`, `flux_frame_submit`,
    /// `flux_surface_read_pixels`, …) so the daemon's watchdog sees progress
    /// when GPU work or readback is slow. Heartbeating between sub-steps
    /// distinguishes "slow but progressing" from a true hang; a genuinely
    /// deadlocked FFI call stops heartbeating too and the watchdog still
    /// fires. Callers pass `&|| wd!().heartbeat()` from the loop; tests pass
    /// `&|| {}`.
    ///
    /// `before_present` is invoked immediately before
    /// `flux_frame_present`. On the offscreen path this is a no-op, but the
    /// stage marker is retained so timing logs and watchdog state keep a
    /// stable boundary between GPU submission and readback/SHM attach. Tests
    /// pass `&|| {}`.
    /// Draw the candidate panel and present it to the compositor.
    ///
    /// Returns `true` iff a frame was actually attached to the surface (i.e.
    /// it will become visible). Returns `false` when the frame was dropped
    /// *before* reaching the compositor — either an early-out on a failed
    /// flux frame begin/submit, or a present-path drop (dma-buf buffer still
    /// held by the compositor, or the SHM pool exhausted). The event loop
    /// treats `false` as "not presented": it keeps the schedule `Dirty` and
    /// retries the frame next tick with the latest coalesced candidates,
    /// instead of marking that composition_seq done and forgetting it.
    pub fn draw_candidates(
        &mut self,
        candidates: &[String],
        selected: usize,
        composition_seq: u64,
        heartbeat: &dyn Fn(),
        before_present: &dyn Fn(),
    ) -> bool {
        let timing_trace_enabled =
            tracing::enabled!(target: PANEL_TIMING_TARGET, tracing::Level::TRACE);
        let timing_info_enabled =
            tracing::enabled!(target: PANEL_TIMING_TARGET, tracing::Level::INFO);
        // The panel probe reuses the same per-stage timing + glyph-stats
        // machinery, but only when its tracing target is enabled. See
        // `emit_panel_probe`.
        let probe_enabled = panel_probe_enabled();
        let timing_enabled = timing_trace_enabled || timing_info_enabled || probe_enabled;
        let total_start = timing_enabled.then(Instant::now);
        let frame_id = if timing_enabled {
            use std::sync::atomic::{AtomicU64, Ordering};
            static FRAME_ID: AtomicU64 = AtomicU64::new(1);
            Some(FRAME_ID.fetch_add(1, Ordering::Relaxed))
        } else {
            None
        };
        let stats_before = if timing_enabled {
            Some(unsafe { text_stats_snapshot(self.text) })
        } else {
            None
        };
        let mut begin_frame_duration = Duration::ZERO;
        let mut canvas_begin_duration = Duration::ZERO;
        let mut measure_duration = Duration::ZERO;
        let mut draw_duration = Duration::ZERO;
        let mut submit_duration = Duration::ZERO;
        let mut present_duration = Duration::ZERO;
        let mut readback_duration = Duration::ZERO;

        macro_rules! timed {
            ($slot:ident, $expr:expr) => {{
                if timing_enabled {
                    let start = Instant::now();
                    let value = $expr;
                    $slot += start.elapsed();
                    value
                } else {
                    $expr
                }
            }};
        }

        heartbeat();
        // Resolve per-candidate metrics up front via the shared layout
        // cache. The cache hit path (canonical Up/Down arrow case: same
        // candidate strings, only `selected` moved) skips the
        // `flux_text_measure` FFI loop entirely. Done outside the
        // `unsafe` block because `layout_candidates` takes `&mut self`
        // and so cannot share the borrow with the FFI calls below.
        let layout = timed!(measure_duration, self.layout_candidates(candidates));
        let presented = unsafe {
            flux_arena_reset(&mut self.arena);
            heartbeat();

            let frame_desc = flux_frame_begin_desc {
                type_: FType::FLUX_TYPE_FRAME_BEGIN_DESC,
                next: ptr::null(),
                timeout_ns: PANEL_FRAME_TIMEOUT_NS,
            };
            let mut frame: *mut flux_frame = ptr::null_mut();
            let r = timed!(
                begin_frame_duration,
                flux_surface_begin_frame(self.surface, &frame_desc, &mut frame)
            );
            heartbeat();
            if !flux_result_is_ok(r) {
                return false;
            }

            // Transparent clear: the rounded-rect fill below covers the body,
            // leaving the four corners outside the arc at alpha 0 so the
            // compositor blends them away and the panel reads as a floating
            // rounded rectangle. The SHM path attaches ARGB buffers, so alpha
            // is preserved without a WSI composite-alpha mode.
            let clear_color = flux_color_rgba(0, 0, 0, 0);
            let r = timed!(
                canvas_begin_duration,
                flux_canvas_begin(self.canvas, frame, &clear_color)
            );
            heartbeat();
            if !flux_result_is_ok(r) {
                return false;
            }

            timed!(draw_duration, self.draw_panel_background());
            heartbeat();
            if !flux_result_is_ok(r) {
                return false;
            }

            let text_color = flux_color_rgba(240, 240, 240, 255);
            let number_color = flux_color_rgba(145, 145, 152, 255);
            let highlight = flux_color_rgba(56, 84, 160, 255);

            let style = flux_text_sys::flux_text_style {
                size_px: CANDIDATE_FONT_SIZE,
                weight: 400.0,
                color: text_color,
                family: FontFamily::FLUX_TEXT_FAMILY_DEFAULT,
                italic: false,
            };
            let number_style = flux_text_sys::flux_text_style {
                size_px: CANDIDATE_NUMBER_FONT_SIZE,
                weight: 400.0,
                color: number_color,
                family: FontFamily::FLUX_TEXT_FAMILY_DEFAULT,
                italic: false,
            };

            let mut current_x = PANEL_PADDING;
            let y = PANEL_PADDING;

            for (i, candidate) in candidates.iter().enumerate() {
                let number_bytes = candidate_number_label(i);
                let bytes = candidate.as_bytes();
                let (number_metrics, metrics) = layout[i];

                let item_width = CANDIDATE_ITEM_X_PADDING * 2.0
                    + number_metrics.width
                    + CANDIDATE_NUMBER_GAP
                    + metrics.width;
                let text_top = y + (PANEL_ROW_HEIGHT - metrics.height).max(0.0) / 2.0;
                let number_top = text_top + metrics.baseline - number_metrics.baseline;

                if i == selected {
                    match self.style {
                        PanelStyle::Classic => {
                            timed!(
                                draw_duration,
                                flux_sys::flux_canvas_fill_rrect(
                                    self.canvas,
                                    flux_sys::flux_rect {
                                        x: current_x,
                                        y,
                                        w: item_width,
                                        h: PANEL_ROW_HEIGHT,
                                    },
                                    4.0,
                                    highlight,
                                )
                            );
                        }
                        PanelStyle::LiquidGlass => {
                            let item_rect = flux_sys::flux_rect {
                                x: current_x,
                                y,
                                w: item_width,
                                h: PANEL_ROW_HEIGHT,
                            };
                            let item_radius = 6.0;

                            // 1. Deep Drop Shadow for the pill (enhances float and 3D volume)
                            let shadow_rect = flux_sys::flux_rect {
                                x: current_x,
                                y: y + 3.0,
                                w: item_width,
                                h: PANEL_ROW_HEIGHT,
                            };
                            timed!(
                                draw_duration,
                                flux_sys::flux_canvas_fill_rrect(
                                    self.canvas,
                                    shadow_rect,
                                    item_radius,
                                    flux_sys::flux_color_rgba_premul(0, 0, 0, 90),
                                )
                            );

                            // 2. Vibrant Volume Fill for Selection (Glossy Liquid Blue)
                            let mut shape: *mut flux_sys::flux_path = std::ptr::null_mut();
                            flux_sys::flux_path_create(&mut shape, &mut self.arena);
                            if !shape.is_null() {
                                flux_sys::flux_path_add_round_rect(shape, item_rect, item_radius);

                                // Beautiful gradient blue base with strong depth
                                let mut vol_stops = [
                                    flux_sys::flux_gradient_stop {
                                        t: 0.00,
                                        color: flux_sys::flux_color_rgba_premul(80, 160, 255, 230),
                                    },
                                    flux_sys::flux_gradient_stop {
                                        t: 0.50,
                                        color: flux_sys::flux_color_rgba_premul(50, 120, 245, 210),
                                    },
                                    flux_sys::flux_gradient_stop {
                                        t: 1.00,
                                        color: flux_sys::flux_color_rgba_premul(20, 80, 200, 190),
                                    },
                                ];
                                let mut vol = flux_sys::flux_paint_linear_gradient(
                                    flux_sys::flux_point {
                                        x: item_rect.x,
                                        y: item_rect.y,
                                    },
                                    flux_sys::flux_point {
                                        x: item_rect.x,
                                        y: item_rect.y + item_rect.h,
                                    },
                                    vol_stops.as_mut_ptr(),
                                    3,
                                );
                                timed!(
                                    draw_duration,
                                    flux_sys::flux_canvas_fill_path(self.canvas, shape, &mut vol)
                                );

                                // 3. Thick Inner Fresnel Bevel (simulating refraction)
                                let mut bevel_shape: *mut flux_sys::flux_path =
                                    std::ptr::null_mut();
                                flux_sys::flux_path_create(&mut bevel_shape, &mut self.arena);
                                if !bevel_shape.is_null() {
                                    let width = 2.0;
                                    let inset = width / 2.0;
                                    let mut inner_rect = item_rect;
                                    inner_rect.x += inset;
                                    inner_rect.y += inset;
                                    inner_rect.w -= width;
                                    inner_rect.h -= width;
                                    flux_sys::flux_path_add_round_rect(
                                        bevel_shape,
                                        inner_rect,
                                        item_radius - inset,
                                    );

                                    let mut bevel_stops = [
                                        flux_sys::flux_gradient_stop {
                                            t: 0.00,
                                            color: flux_sys::flux_color_rgba_premul(
                                                180, 220, 255, 140,
                                            ),
                                        },
                                        flux_sys::flux_gradient_stop {
                                            t: 0.40,
                                            color: flux_sys::flux_color_rgba_premul(
                                                150, 200, 255, 0,
                                            ),
                                        },
                                        flux_sys::flux_gradient_stop {
                                            t: 0.70,
                                            color: flux_sys::flux_color_rgba_premul(0, 0, 0, 0),
                                        },
                                        flux_sys::flux_gradient_stop {
                                            t: 1.00,
                                            color: flux_sys::flux_color_rgba_premul(0, 0, 0, 70),
                                        },
                                    ];
                                    let mut sp = flux_sys::flux_paint_linear_gradient(
                                        flux_sys::flux_point {
                                            x: item_rect.x,
                                            y: item_rect.y,
                                        },
                                        flux_sys::flux_point {
                                            x: item_rect.x,
                                            y: item_rect.y + item_rect.h,
                                        },
                                        bevel_stops.as_mut_ptr(),
                                        4,
                                    );
                                    sp.stroke_width = width;
                                    sp.join = flux_sys::flux_line_join::FLUX_JOIN_ROUND;
                                    timed!(
                                        draw_duration,
                                        flux_sys::flux_canvas_stroke_path(
                                            self.canvas,
                                            bevel_shape,
                                            &sp
                                        )
                                    );
                                }

                                // 4. Top Specular Glass Reflection (Sharp gloss on top half)
                                let mut sheen_stops = [
                                    flux_sys::flux_gradient_stop {
                                        t: 0.00,
                                        color: flux_sys::flux_color_rgba_premul(255, 255, 255, 160),
                                    },
                                    flux_sys::flux_gradient_stop {
                                        t: 0.45,
                                        color: flux_sys::flux_color_rgba_premul(255, 255, 255, 15),
                                    },
                                    flux_sys::flux_gradient_stop {
                                        t: 0.50,
                                        color: flux_sys::flux_color_rgba_premul(255, 255, 255, 0),
                                    },
                                ];
                                let mut sheen = flux_sys::flux_paint_linear_gradient(
                                    flux_sys::flux_point {
                                        x: item_rect.x,
                                        y: item_rect.y,
                                    },
                                    flux_sys::flux_point {
                                        x: item_rect.x,
                                        y: item_rect.y + item_rect.h,
                                    },
                                    sheen_stops.as_mut_ptr(),
                                    3,
                                );
                                timed!(
                                    draw_duration,
                                    flux_sys::flux_canvas_fill_path(self.canvas, shape, &mut sheen)
                                );

                                // 5. Hairline Surface Tension
                                let mut inner_stops = [
                                    flux_sys::flux_gradient_stop {
                                        t: 0.00,
                                        color: flux_sys::flux_color_rgba_premul(255, 255, 255, 230),
                                    },
                                    flux_sys::flux_gradient_stop {
                                        t: 0.50,
                                        color: flux_sys::flux_color_rgba_premul(255, 255, 255, 30),
                                    },
                                    flux_sys::flux_gradient_stop {
                                        t: 1.00,
                                        color: flux_sys::flux_color_rgba_premul(255, 255, 255, 80),
                                    },
                                ];
                                let mut inner_sp = flux_sys::flux_paint_linear_gradient(
                                    flux_sys::flux_point {
                                        x: item_rect.x,
                                        y: item_rect.y,
                                    },
                                    flux_sys::flux_point {
                                        x: item_rect.x + item_rect.w,
                                        y: item_rect.y + item_rect.h,
                                    },
                                    inner_stops.as_mut_ptr(),
                                    3,
                                );
                                inner_sp.stroke_width = 1.0;
                                inner_sp.join = flux_sys::flux_line_join::FLUX_JOIN_ROUND;
                                timed!(
                                    draw_duration,
                                    flux_sys::flux_canvas_stroke_path(
                                        self.canvas,
                                        shape,
                                        &inner_sp
                                    )
                                );
                            }
                        }
                    }
                }

                timed!(
                    draw_duration,
                    flux_text_draw(
                        self.text,
                        self.canvas as *mut flux_text_sys::flux_canvas,
                        &mut self.arena as *mut flux_arena as *mut flux_text_sys::flux_arena,
                        current_x + CANDIDATE_ITEM_X_PADDING,
                        number_top,
                        number_bytes.as_ptr() as *const _,
                        number_bytes.len(),
                        &number_style,
                    )
                );
                timed!(
                    draw_duration,
                    flux_text_draw(
                        self.text,
                        self.canvas as *mut flux_text_sys::flux_canvas,
                        &mut self.arena as *mut flux_arena as *mut flux_text_sys::flux_arena,
                        current_x
                            + CANDIDATE_ITEM_X_PADDING
                            + number_metrics.width
                            + CANDIDATE_NUMBER_GAP,
                        text_top,
                        bytes.as_ptr() as *const _,
                        bytes.len(),
                        &style,
                    )
                );

                current_x += item_width + CANDIDATE_ITEM_GAP;
            }
            heartbeat();

            flux_canvas_end(self.canvas);
            heartbeat();
            let r = timed!(submit_duration, flux_frame_submit(frame));
            heartbeat();
            if !flux_result_is_ok(r) {
                return false;
            }
            before_present();
            // Offscreen path: present is a no-op (no swapchain). The actual
            // "present" is either a dma-buf export (zero-copy) or a readback +
            // shm attach, below.
            timed!(present_duration, flux_frame_present(frame));
            heartbeat();

            if self.dmabuf_pool.is_some() {
                // ── Zero-copy dma-buf path ───────────────────────────────
                // Export the submitted frame's GPU memory as a dma-buf fd and
                // hand it to the compositor. No GPU→CPU pixel copy; the only
                // wait is the frame fence (GPU render completion), same as
                // read_pixels but without the staging buffer copy.
                let mut fd: i32 = -1;
                let readback_start = Instant::now();
                let r = flux_sys::flux_surface_export_dmabuf(self.surface, &mut fd);
                if timing_enabled {
                    readback_duration = readback_start.elapsed();
                }
                heartbeat();
                if !flux_result_is_ok(r) {
                    tracing::warn!(
                        target: "typio.panel.dmabuf",
                        "flux_surface_export_dmabuf failed"
                    );
                    return false;
                }
                let owned_fd = std::os::fd::FromRawFd::from_raw_fd(fd);
                let presented = self.present_dmabuf(owned_fd);
                heartbeat();
                self.log_text_stats();
                presented
            } else {
                // ── Readback + SHM path (fallback) ───────────────────────
                // Read the rendered frame back from the GPU. Bounded by the
                // frame's fence (GPU completion) — never by the compositor.
                let pixel_bytes = (self.width as usize) * (self.height as usize) * 4;
                let mut readback_buf: Vec<u8> = vec![0u8; pixel_bytes];
                let readback_start = Instant::now();
                let r = flux_surface_read_pixels(
                    self.surface,
                    readback_buf.as_mut_ptr() as *mut c_void,
                    pixel_bytes,
                );
                if timing_enabled {
                    readback_duration = readback_start.elapsed();
                }
                heartbeat();
                if !flux_result_is_ok(r) {
                    tracing::warn!(
                        target: "typio.panel.shm",
                        "flux_surface_read_pixels failed"
                    );
                    return false;
                }

                // Hand the pixels to the compositor via a host-managed shm
                // buffer. acquire returns None when all buffers are busy
                // (compositor hasn't released them) — drop this frame rather
                // than block. This is the structural fix: a slow compositor
                // causes dropped frames, never a deadlock.
                let presented = self.present_shm(&readback_buf);
                heartbeat();
                self.log_text_stats();
                presented
            }
        };
        // Timing emission happens after the present result is known so the
        // slow-frame log can also report whether the frame actually reached
        // the compositor.
        if let Some(total_start) = total_start {
            let total_duration = total_start.elapsed();
            let slow = total_duration >= PANEL_TIMING_SLOW_THRESHOLD;
            if slow || timing_trace_enabled {
                let stats_after = unsafe { text_stats_snapshot(self.text) };
                let stats_before = stats_before.unwrap_or_default();
                macro_rules! emit_panel_timing {
                    ($level:ident) => {
                        tracing::$level!(
                            target: PANEL_TIMING_TARGET,
                            frame_id = frame_id.unwrap_or(0),
                            composition_seq,
                            candidate_count = candidates.len(),
                            selected,
                            slow,
                            slow_threshold_ms = ms(PANEL_TIMING_SLOW_THRESHOLD),
                            total_ms = ms(total_duration),
                            size_ms = ms(self.last_candidate_size_duration),
                            resized = self.last_candidate_size_resized,
                            begin_ms = ms(begin_frame_duration),
                            canvas_ms = ms(canvas_begin_duration),
                            measure_ms = ms(measure_duration),
                            draw_ms = ms(draw_duration),
                            submit_ms = ms(submit_duration),
                            present_ms = ms(present_duration),
                            readback_ms = ms(readback_duration),
                            glyph_count = stats_after.glyph_count,
                            glyph_cap = stats_after.glyph_cap,
                            glyph_hits_delta = stats_after
                                .glyph_hits
                                .saturating_sub(stats_before.glyph_hits),
                            glyph_misses_delta = stats_after
                                .glyph_misses
                                .saturating_sub(stats_before.glyph_misses),
                            glyph_evictions_delta = stats_after
                                .glyph_evictions
                                .saturating_sub(stats_before.glyph_evictions),
                            atlas_clears_delta = stats_after
                                .atlas_clears
                                .saturating_sub(stats_before.atlas_clears),
                            surface_width = self.width,
                            surface_height = self.height,
                            has_viewport = self.viewport.is_some(),
                            "panel frame"
                        );
                    };
                }
                if slow {
                    emit_panel_timing!(info);
                } else {
                    emit_panel_timing!(trace);
                }
            }
            if probe_enabled {
                let stats_after = unsafe { text_stats_snapshot(self.text) };
                emit_panel_probe(
                    present_duration,
                    total_duration,
                    candidates.len(),
                    &stats_after,
                );
            }
        }
        presented
    }

    /// Emit glyph-cache + atlas stats to stderr. Throttled to once every
    /// 120 candidate frames, or immediately when `atlas_clears` rises
    /// (the signal that the atlas saturated and the next frame must
    /// re-rasterise every visible glyph).
    fn log_text_stats(&self) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static FRAME: AtomicU64 = AtomicU64::new(0);
        static LAST_CLEARS: AtomicU64 = AtomicU64::new(0);
        let n = FRAME.fetch_add(1, Ordering::Relaxed);
        let mut stats: flux_text_sys::flux_text_stats = unsafe { std::mem::zeroed() };
        unsafe {
            flux_text_sys::flux_text_get_stats(self.text, &mut stats);
        }
        let clears = stats.atlas_clears;
        let last = LAST_CLEARS.load(Ordering::Relaxed);
        if clears != last || n % 120 == 0 {
            LAST_CLEARS.store(clears, Ordering::Relaxed);
            tracing::debug!(
                target: "typio.panel.text",
                frame = n,
                glyph_count = stats.glyph_count,
                glyph_max_cap = stats.glyph_max_cap,
                glyph_cap = stats.glyph_cap,
                glyph_hits = stats.glyph_hits,
                glyph_misses = stats.glyph_misses,
                glyph_evictions = stats.glyph_evictions,
                glyph_invalidations = stats.glyph_invalidations,
                glyph_grows = stats.glyph_grows,
                atlas_clears = stats.atlas_clears,
                "flux text stats"
            );
        }
    }

    /// Resize the panel surface.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let changed = self.width != width || self.height != height;
        self.width = width;
        self.height = height;
        if changed && !self.surface.is_null() {
            unsafe {
                flux_sys::flux_surface_resize(self.surface, width, height);
            }
            // Surface resize recreates the offscreen images, so the dma-buf
            // pool's wl_buffers (which wrap the old images' memory) are now
            // stale. Clear them; they'll be recreated at the new size on the
            // next present.
            if let Some(pool) = self.dmabuf_pool.as_mut() {
                pool.clear();
            }
        }
    }

    /// Return cached `(number_metrics, text_metrics)` per candidate,
    /// re-running `flux_text_measure` only when the candidate strings or
    /// the rendering scale have changed since the last call.
    ///
    /// Both [`Self::ensure_candidate_size`] (total-width sizing) and
    /// [`Self::draw_candidates`] (per-item placement) consult this cache,
    /// so a highlight-only update — the canonical Up/Down arrow case,
    /// where the candidate set is unchanged and only `selected` differs —
    /// skips the `flux_text_measure` FFI loop entirely.
    fn layout_candidates(
        &mut self,
        candidates: &[String],
    ) -> Vec<(flux_text_metrics, flux_text_metrics)> {
        let cache_valid = self
            .last_layout_key
            .as_ref()
            .map(|(cached, scale)| {
                *scale == self.scale && cached.len() == candidates.len() && {
                    cached.iter().zip(candidates.iter()).all(|(a, b)| a == b)
                }
            })
            .unwrap_or(false);
        if cache_valid {
            return self.last_layout.clone();
        }

        let style = flux_text_sys::flux_text_style {
            size_px: CANDIDATE_FONT_SIZE,
            weight: 400.0,
            color: unsafe { flux_sys::flux_color_rgba(240, 240, 240, 255) },
            family: FontFamily::FLUX_TEXT_FAMILY_DEFAULT,
            italic: false,
        };
        let number_style = flux_text_sys::flux_text_style {
            size_px: CANDIDATE_NUMBER_FONT_SIZE,
            weight: 400.0,
            color: unsafe { flux_sys::flux_color_rgba(145, 145, 152, 255) },
            family: FontFamily::FLUX_TEXT_FAMILY_DEFAULT,
            italic: false,
        };

        let mut out: Vec<(flux_text_metrics, flux_text_metrics)> =
            Vec::with_capacity(candidates.len());
        for (i, candidate) in candidates.iter().enumerate() {
            let number_bytes = candidate_number_label(i);
            let number_metrics = unsafe {
                flux_text_sys::flux_text_measure(
                    self.text,
                    number_bytes.as_ptr() as *const _,
                    number_bytes.len(),
                    &number_style,
                )
            };
            let bytes = candidate.as_bytes();
            let metrics = unsafe {
                flux_text_sys::flux_text_measure(
                    self.text,
                    bytes.as_ptr() as *const _,
                    bytes.len(),
                    &style,
                )
            };
            out.push((number_metrics, metrics));
        }
        self.last_layout_key = Some((candidates.to_vec(), self.scale));
        self.last_layout = out.clone();
        out
    }

    /// Ensure the surface is big enough for `candidate_count` rows.
    ///
    /// Two paths, per ADR-0013 (adapted to the offscreen + SHM render path):
    ///
    /// - **With `wp_viewport` (preferred).** The offscreen image is
    ///   quantised up to `SURFACE_WIDTH_QUANTUM` and grows only. A width
    ///   change inside the current quantum reuses the existing offscreen
    ///   image — no `flux_surface_resize` — and the exact content rect is
    ///   cropped via `wp_viewport.set_source` / `set_destination`. After a
    ///   short warm-up the image reaches the widest candidate row and
    ///   `flux_surface_resize` is never called again during steady-state
    ///   paging.
    ///
    /// - **Without `wp_viewport` (fallback).** The SHM buffer must equal the
    ///   content exactly (the buffer maps 1:1 to the surface), so any width
    ///   change reallocates the offscreen image. This is costlier than the
    ///   viewport path but no longer watchdog-killing — there is no WSI
    ///   swapchain to rebuild.
    pub fn ensure_candidate_size(&mut self, candidates: &[String]) {
        let timing_enabled = tracing::enabled!(target: PANEL_TIMING_TARGET, tracing::Level::INFO)
            || tracing::enabled!(target: PANEL_TIMING_TARGET, tracing::Level::TRACE);
        let start = timing_enabled.then(Instant::now);

        // Hit the shared layout cache so the canonical arrow-key
        // navigation case (same candidates, only `selected` moved)
        // skips the `flux_text_measure` FFI loop entirely.
        let layout = self.layout_candidates(candidates);
        let mut total_width: f32 = PANEL_PADDING;
        for (number_metrics, metrics) in layout.iter() {
            let item_width = CANDIDATE_ITEM_X_PADDING * 2.0
                + number_metrics.width
                + CANDIDATE_NUMBER_GAP
                + metrics.width;
            total_width += item_width + CANDIDATE_ITEM_GAP;
        }

        if !candidates.is_empty() {
            total_width = total_width - CANDIDATE_ITEM_GAP + PANEL_PADDING;
        } else {
            total_width += PANEL_PADDING;
        }

        let desired_width = (total_width as u32).max(10);
        let desired_height = (PANEL_PADDING * 2.0 + PANEL_ROW_HEIGHT).ceil() as u32;

        let phys_width = (desired_width as f32 * self.scale).ceil() as u32;
        let phys_height = (desired_height as f32 * self.scale).ceil() as u32;

        let content_w_logical = desired_width as i32;
        let content_h_logical = desired_height as i32;

        self.last_candidate_size_resized = self.apply_grow_only_size(
            phys_width,
            phys_height,
            content_w_logical,
            content_h_logical,
        );
        if let Some(start) = start {
            self.last_candidate_size_duration = start.elapsed();
        }
    }

    /// Grow-only offscreen-image sizing shared by the candidate and banner
    /// paths (ADR-0013, extended to height). Both axes are quantised
    /// up and never shrink, so any content change that stays inside
    /// the current quantum reuses the existing offscreen image and only
    /// re-issues a `wp_viewport` crop. This keeps `flux_surface_resize`
    /// out of the steady-state render path, including the first indicator
    /// banner of every fresh daemon.
    ///
    /// Falls back to exact-size `resize()` when the compositor lacks
    /// `wp_viewporter`; that path resizes the offscreen image on every
    /// content-size change.
    fn apply_grow_only_size(
        &mut self,
        phys_width: u32,
        phys_height: u32,
        content_w_logical: i32,
        content_h_logical: i32,
    ) -> bool {
        if let Some(viewport) = self.viewport.as_ref() {
            let mut resized = false;
            let quantised_phys_w =
                phys_width.div_ceil(SURFACE_WIDTH_QUANTUM) * SURFACE_WIDTH_QUANTUM;
            let quantised_phys_h =
                phys_height.div_ceil(SURFACE_HEIGHT_QUANTUM) * SURFACE_HEIGHT_QUANTUM;
            let target_phys_w = self.width.max(quantised_phys_w);
            let target_phys_h = self.height.max(quantised_phys_h);
            if target_phys_w != self.width || target_phys_h != self.height {
                resized = true;
                self.width = target_phys_w;
                self.height = target_phys_h;
                if !self.surface.is_null() {
                    unsafe {
                        flux_sys::flux_surface_resize(self.surface, target_phys_w, target_phys_h);
                    }
                }
            }
            // Re-issue the crop whenever either side of the viewport mapping
            // changes. `wp_viewport.set_source` takes buffer (physical)
            // coordinates; `set_destination` takes surface (logical)
            // coordinates. A cross-output scale change can keep the logical
            // destination identical while changing the physical source, so
            // caching only the destination leaves the compositor clipping the
            // old 1x source rectangle out of a newly 2x-rendered buffer.
            if viewport_mapping_changed(
                self.viewport_source_w_physical,
                self.viewport_source_h_physical,
                self.content_w_logical,
                self.content_h_logical,
                phys_width,
                phys_height,
                content_w_logical,
                content_h_logical,
            ) {
                viewport.set_source(0.0, 0.0, phys_width as f64, phys_height as f64);
                viewport.set_destination(content_w_logical, content_h_logical);
                self.viewport_source_w_physical = phys_width;
                self.viewport_source_h_physical = phys_height;
                self.content_w_logical = content_w_logical;
                self.content_h_logical = content_h_logical;
            }
            resized
        } else {
            // Legacy path: buffer must equal content exactly.
            let resized = self.width != phys_width || self.height != phys_height;
            self.resize(phys_width, phys_height);
            resized
        }
    }

    /// Hide the panel by detaching the current Wayland buffer.
    pub fn hide(&mut self) {
        unsafe {
            wl_surface_detach_and_commit(self.wl_surface);
        }
    }

    /// Attach the rendered pixels to the panel surface via a host-managed
    /// shm buffer.
    ///
    /// Acquires a free buffer from the pool (dropping this frame if all are
    /// busy — the compositor hasn't released them yet), copies the readback
    /// into it, and issues `wl_surface.attach` + `damage` + `commit`. These
    /// are plain Wayland requests that return instantly; the compositor
    /// processes them asynchronously and sends `wl_buffer.release` when the
    /// buffer is reusable. Unlike `vkQueuePresentKHR`, this can never block
    /// the main thread.
    /// Present via dma-buf: export the frame's GPU memory, ensure a
    /// `wl_buffer` exists for the submitted slot, and attach it to the
    /// surface. Zero-copy — the compositor composites the GPU memory
    /// directly.
    /// Present the submitted frame's GPU memory to the compositor.
    ///
    /// Returns `true` iff a buffer was actually attached to the surface.
    /// `false` means the frame was dropped *without* reaching the
    /// compositor (no dma-buf pool, stride-0/unexportable surface, or the
    /// target slot's previous buffer is still held by the compositor). The
    /// caller must treat `false` as "not presented" so the frame is retried
    /// on a later tick instead of being silently forgotten — otherwise the
    /// schedule moves to Idle and that composition_seq is never redrawn.
    fn present_dmabuf(&mut self, fd: std::os::fd::OwnedFd) -> bool {
        let Some(pool) = self.dmabuf_pool.as_mut() else {
            return false;
        };

        // Query the surface's export metadata (modifier + stride), set at
        // image-creation time by flux.
        let modifier = unsafe { flux_sys::flux_surface_dmabuf_modifier(self.surface) };
        let stride = unsafe { flux_sys::flux_surface_dmabuf_stride(self.surface) };
        if stride == 0 {
            tracing::warn!(
                target: "typio.panel.dmabuf",
                "dmabuf stride is 0 — surface not exportable?"
            );
            return false;
        }

        // Which frame slot was just submitted? With frames_in_flight=2 the GPU
        // alternates between two image slots; each needs its own wl_buffer so
        // the compositor can hold slot N-1 while we submit slot N.
        let slot = unsafe { flux_sys::flux_surface_last_slot(self.surface) } as usize;

        // Ensure a buffer exists for this slot (create or reuse on resize).
        pool.ensure_slot(slot, fd, self.width, self.height, stride, modifier);

        if let Some(buf) = pool.get(slot) {
            if buf.busy() {
                // Compositor still holds this slot's previous frame — drop.
                // (debug, not trace: a dropped candidate frame is a real
                // user-visible regression, not internal noise. The loop's
                // retry path keeps the schedule Dirty so the frame is
                // re-attempted next tick instead of being lost.)
                tracing::debug!(
                    target: "typio.panel.dmabuf",
                    slot,
                    "dmabuf buffer busy — dropping frame (will retry)"
                );
                return false;
            }
            buf.mark_busy();
            // Force buffer_scale=1 when wp_viewport is active, exactly as the
            // SHM path does: the viewport's set_source (physical coords) +
            // set_destination (logical coords) alone map the buffer. The
            // compositor's PreferredBufferScale(2) would otherwise shrink the
            // buffer to buffer_size/2 and make the viewport source exceed it
            // (protocol error 2).
            let desired_scale = if self.viewport.is_some() { 1 } else { self.scale as i32 };
            unsafe {
                wl_surface_set_buffer_scale(self.wl_surface, desired_scale);
            }
            unsafe {
                wl_surface_attach_commit(
                    self.wl_surface,
                    buf.wl_buffer().id().as_ptr() as *mut c_void,
                    self.width,
                    self.height,
                );
            }
            true
        } else {
            // No buffer exists for this slot yet (first present after a
            // resize/clear that drained the pool). ensure_slot above only
            // creates when needs_create is true *and* it can; a None here
            // means the pool has no buffer for this slot at all — drop, and
            // let the retry path re-attempt once ensure_slot has rebuilt it.
            tracing::debug!(
                target: "typio.panel.dmabuf",
                slot,
                "no dmabuf buffer for slot — dropping frame (will retry)"
            );
            false
        }
    }

    /// Present rendered pixels to the compositor via a host-managed SHM buffer.
    ///
    /// Returns `true` iff a buffer was actually attached. `false` means the
    /// frame was dropped *without* reaching the compositor (no SHM pool, or
    /// all buffers busy because the compositor hasn't released them yet). The
    /// caller treats `false` as "not presented" so the loop retries the frame
    /// instead of marking it done.
    fn present_shm(&mut self, pixels: &[u8]) -> bool {
        let Some(pool) = self.shm_pool.as_mut() else {
            return false;
        };
        let Some(idx) = pool.acquire(self.width, self.height) else {
            // All buffers busy (compositor hasn't released them) — drop this
            // frame rather than block. (debug, not trace: same rationale as
            // the dmabuf path — a dropped candidate frame is user-visible.)
            tracing::debug!(
                target: "typio.panel.shm",
                "shm buffer pool exhausted — dropping frame (will retry)"
            );
            return false;
        };
        let buf = pool.get(idx).expect("acquire returned a valid index");
        let dst = buf.pixels();
        let copy_len = pixels.len().min(buf.pixel_len());
        // flux offscreen renders B8G8R8A8_UNORM, which matches Wayland
        // ARGB8888 byte order (B,G,R,A on little-endian) directly — no
        // channel swap needed. Plain memcpy.
        unsafe {
            ptr::copy_nonoverlapping(pixels.as_ptr(), dst, copy_len);
        }
        buf.mark_busy();
        // When wp_viewport is active, force buffer_scale=1 so the viewport's
        // set_source (physical buffer coords) + set_destination (logical
        // surface coords) alone map the physical-pixel shm buffer to logical
        // size. The compositor may have sent PreferredBufferScale earlier
        // (handled in Dispatch<wl_surface>), which set buffer_scale=2; that
        // makes the compositor see the buffer at buffer_size/scale, and the
        // physical source rectangle then exceeds that shrunken area
        // (protocol error 2). Reset to 1 every present to override it.
        // On the legacy no-viewport path, set buffer_scale to self.scale so
        // the exact-sized physical buffer is interpreted at the right logical
        // size.
        let desired_scale = if self.viewport.is_some() {
            1
        } else {
            self.scale as i32
        };
        unsafe {
            wl_surface_set_buffer_scale(self.wl_surface, desired_scale);
        }
        // Attach + damage + commit via raw FFI (the wl_surface is a raw
        // pointer from wayland-sys; we don't have a safe WlSurface handle
        // here). These requests are queued and flushed by the event loop;
        // none of them block.
        unsafe {
            wl_surface_attach_commit(
                self.wl_surface,
                buf.wl_buffer().id().as_ptr() as *mut c_void,
                self.width,
                self.height,
            );
        }
        true
    }

    /// Draw the status banner — a single centred text label used by the
    /// indicator (engine · mode feedback) and voice status overlays. Shares
    /// the candidate panel's offscreen surface and flux text stack per
    /// ADR-0017 (one positioned popup surface, mutually exclusive owners).
    ///
    /// Empty labels are ignored — caller should `hide()` instead.
    ///
    /// `heartbeat` mirrors [`FluxPanel::draw_candidates`]: invoked
    /// between blocking FFI calls so a slow compositor does not trip
    /// the watchdog. `before_present` is invoked immediately before
    /// `flux_frame_present` so the caller can transition the watchdog
    /// to the longer-threshold `Present` stage; see
    /// [`FluxPanel::draw_candidates`] for the rationale.
    /// Draw the status banner and present it to the compositor.
    ///
    /// Returns `true` iff a frame was actually attached to the surface. See
    /// [`FluxPanel::draw_candidates`] for the `false` contract — callers
    /// (`render_indicator_banner` / `render_voice_status_banner`) currently
    /// ignore the result because banners are fire-and-forget (re-shown on the
    /// next trigger), but the contract is kept symmetric so a future retry
    /// path is a drop-in.
    ///
    /// Empty labels are ignored (return `false`) — caller should `hide()`
    /// instead.
    ///
    /// `heartbeat` mirrors [`FluxPanel::draw_candidates`]: invoked
    /// between blocking FFI calls so a slow compositor does not trip
    /// the watchdog. `before_present` is invoked immediately before
    /// `flux_frame_present` so the caller can transition the watchdog
    /// to the longer-threshold `Present` stage; see
    /// [`FluxPanel::draw_candidates`] for the rationale.
    pub fn draw_status_banner(
        &mut self,
        label: &str,
        heartbeat: &dyn Fn(),
        before_present: &dyn Fn(),
    ) -> bool {
        if label.is_empty() {
            return false;
        }
        heartbeat();
        let presented = unsafe {
            flux_arena_reset(&mut self.arena);
            heartbeat();

            let frame_desc = flux_frame_begin_desc {
                type_: FType::FLUX_TYPE_FRAME_BEGIN_DESC,
                next: ptr::null(),
                timeout_ns: PANEL_FRAME_TIMEOUT_NS,
            };
            let mut frame: *mut flux_frame = ptr::null_mut();
            let r = flux_surface_begin_frame(self.surface, &frame_desc, &mut frame);
            heartbeat();
            if !flux_result_is_ok(r) {
                false
            } else {
                // Transparent clear so the rounded-banner corners blend away
                // (matches draw_candidates).
                let clear_color = flux_color_rgba(0, 0, 0, 0);
                let r = flux_canvas_begin(self.canvas, frame, &clear_color);
                heartbeat();
                if !flux_result_is_ok(r) {
                    false
                } else {
                    self.draw_panel_background();
                    heartbeat();
                    if !flux_result_is_ok(r) {
                        false
                    } else {
                        self.draw_banner_inner(
                            label,
                            frame,
                            heartbeat,
                            before_present,
                        )
                    }
                }
            }
        };
        presented
    }

    /// Banner rendering + present shared by [`Self::draw_status_banner`].
    /// Assumes the caller has already begun the frame and canvas, drawn the
    /// background, and verified flux result codes. Returns `true` iff a frame
    /// was attached to the surface; `false` on any flux failure or
    /// present-path drop (dmabuf busy / shm exhausted).
    ///
    /// Split out so the ladder of flux early-out checks in
    /// `draw_status_banner` stays shallow instead of a deeply nested
    /// `match`/`else` tower.
    unsafe fn draw_banner_inner(
        &mut self,
        label: &str,
        frame: *mut flux_frame,
        heartbeat: &dyn Fn(),
        before_present: &dyn Fn(),
    ) -> bool {
        unsafe {
            let text_color = flux_color_rgba(240, 240, 240, 255);
            let style = flux_text_sys::flux_text_style {
                size_px: BANNER_FONT_SIZE,
                weight: 400.0,
                color: text_color,
                family: FontFamily::FLUX_TEXT_FAMILY_DEFAULT,
                italic: false,
            };

            let bytes = label.as_bytes();
            let metrics = flux_text_sys::flux_text_measure(
                self.text,
                bytes.as_ptr() as *const _,
                bytes.len(),
                &style,
            );
            heartbeat();

            let text_y =
                BANNER_PADDING + (BANNER_FONT_SIZE * 1.3 - metrics.height).max(0.0) / 2.0;
            let text_x = BANNER_PADDING;

            flux_text_draw(
                self.text,
                self.canvas as *mut flux_text_sys::flux_canvas,
                &mut self.arena as *mut flux_arena as *mut flux_text_sys::flux_arena,
                text_x,
                text_y,
                bytes.as_ptr() as *const _,
                bytes.len(),
                &style,
            );
            heartbeat();

            flux_canvas_end(self.canvas);
            heartbeat();
            let r = flux_frame_submit(frame);
            heartbeat();
            if !flux_result_is_ok(r) {
                return false;
            }
            before_present();
            // Offscreen: present is a no-op; the real "present" is either a
            // dma-buf export or readback + shm attach. Same non-blocking path
            // as draw_candidates.
            flux_frame_present(frame);
            heartbeat();

            if self.dmabuf_pool.is_some() {
                // Zero-copy dma-buf path.
                let mut fd: i32 = -1;
                let r = flux_sys::flux_surface_export_dmabuf(self.surface, &mut fd);
                heartbeat();
                if !flux_result_is_ok(r) {
                    tracing::warn!(
                        target: "typio.panel.dmabuf",
                        "banner flux_surface_export_dmabuf failed"
                    );
                    return false;
                }
                let owned_fd = std::os::fd::FromRawFd::from_raw_fd(fd);
                let presented = self.present_dmabuf(owned_fd);
                heartbeat();
                presented
            } else {
                // Readback + SHM fallback.
                let pixel_bytes = (self.width as usize) * (self.height as usize) * 4;
                let mut readback_buf: Vec<u8> = vec![0u8; pixel_bytes];
                let r = flux_surface_read_pixels(
                    self.surface,
                    readback_buf.as_mut_ptr() as *mut c_void,
                    pixel_bytes,
                );
                heartbeat();
                if !flux_result_is_ok(r) {
                    tracing::warn!(
                        target: "typio.panel.shm",
                        "banner read_pixels failed"
                    );
                    return false;
                }
                let presented = self.present_shm(&readback_buf);
                heartbeat();
                presented
            }
        }
    }

    /// Ensure the surface is big enough for a single-row banner of `label`.
    /// Mirrors `ensure_candidate_size`'s two-path strategy (ADR-0013):
    /// grow-only with `wp_viewport` when available, exact-size resize
    /// otherwise. Banner rows are usually narrower than candidate rows, so
    /// after a candidate-panel showing the offscreen image typically reuses
    /// the existing quantum.
    pub fn ensure_banner_size(&mut self, label: &str) {
        let style = flux_text_sys::flux_text_style {
            size_px: BANNER_FONT_SIZE,
            weight: 400.0,
            // Colour is irrelevant for `flux_text_measure`; provide one to
            // keep the struct fully initialised.
            color: unsafe { flux_color_rgba(0, 0, 0, 0) },
            family: FontFamily::FLUX_TEXT_FAMILY_DEFAULT,
            italic: false,
        };

        let bytes = label.as_bytes();
        let metrics = unsafe {
            flux_text_sys::flux_text_measure(
                self.text,
                bytes.as_ptr() as *const _,
                bytes.len(),
                &style,
            )
        };
        let desired_width = (BANNER_PADDING * 2.0 + metrics.width).max(10.0).ceil() as u32;
        let desired_height = (BANNER_ROW_HEIGHT).ceil() as u32;

        let phys_width = (desired_width as f32 * self.scale).ceil() as u32;
        let phys_height = (desired_height as f32 * self.scale).ceil() as u32;

        let content_w_logical = desired_width as i32;
        let content_h_logical = desired_height as i32;

        self.apply_grow_only_size(
            phys_width,
            phys_height,
            content_w_logical,
            content_h_logical,
        );
    }
}

fn viewport_mapping_changed(
    cached_source_w_physical: u32,
    cached_source_h_physical: u32,
    cached_dest_w_logical: i32,
    cached_dest_h_logical: i32,
    source_w_physical: u32,
    source_h_physical: u32,
    dest_w_logical: i32,
    dest_h_logical: i32,
) -> bool {
    cached_source_w_physical != source_w_physical
        || cached_source_h_physical != source_h_physical
        || cached_dest_w_logical != dest_w_logical
        || cached_dest_h_logical != dest_h_logical
}

impl Drop for FluxPanel {
    fn drop(&mut self) {
        unsafe {
            flux_arena_destroy(&mut self.arena);
            if !self.text.is_null() {
                flux_text_destroy(self.text);
            }
            if !self.canvas.is_null() {
                flux_canvas_destroy(self.canvas);
            }
            if !self.surface.is_null() {
                flux_surface_release(self.surface);
            }
            if !self.device.is_null() {
                flux_device_release(self.device);
            }
            // Free the cache path AFTER flux_device_release so the
            // save callback (fired inside release) can still read it.
            free_cache_path(self.pipeline_cache_path);
        }
    }
}

fn candidate_number_label(index: usize) -> std::borrow::Cow<'static, [u8]> {
    match index {
        0 => std::borrow::Cow::Borrowed(b"1"),
        1 => std::borrow::Cow::Borrowed(b"2"),
        2 => std::borrow::Cow::Borrowed(b"3"),
        3 => std::borrow::Cow::Borrowed(b"4"),
        4 => std::borrow::Cow::Borrowed(b"5"),
        5 => std::borrow::Cow::Borrowed(b"6"),
        6 => std::borrow::Cow::Borrowed(b"7"),
        7 => std::borrow::Cow::Borrowed(b"8"),
        8 => std::borrow::Cow::Borrowed(b"9"),
        9 => std::borrow::Cow::Borrowed(b"0"),
        _ => std::borrow::Cow::Owned((index + 1).to_string().into_bytes()),
    }
}

// ── Raw Wayland surface creation via wayland-sys ──────────────────────────

unsafe fn wl_surface_detach_and_commit(wl_surface: *mut c_void) {
    if wl_surface.is_null() {
        return;
    }

    let surface = wl_surface as *mut wl_proxy;
    let mut attach_args = [
        wl_argument { o: ptr::null() },
        wl_argument { i: 0 },
        wl_argument { i: 0 },
    ];
    unsafe {
        wl_proxy_marshal_array(surface, 1, attach_args.as_mut_ptr());
    }

    let mut commit_args: [wl_argument; 0] = [];
    unsafe {
        wl_proxy_marshal_array(surface, 6, commit_args.as_mut_ptr());
    }
}

/// Set the buffer scale on the surface (wl_surface.set_buffer_scale —
/// opcode 8). Tells the compositor the attached buffer is `scale`× physical
/// pixels per logical pixel.
unsafe fn wl_surface_set_buffer_scale(wl_surface: *mut c_void, scale: i32) {
    if wl_surface.is_null() {
        return;
    }
    let surface = wl_surface as *mut wl_proxy;
    let mut args = [wl_argument { i: scale }];
    unsafe {
        wl_proxy_marshal_array(surface, 8, args.as_mut_ptr());
    }
}

/// Attach `buffer` (a raw `wl_buffer*` = `wl_proxy*`) to the surface, damage
/// the full surface, and commit. These are queued Wayland requests — none of
/// them block. `width`/`height` are the buffer size in physical pixels.
unsafe fn wl_surface_attach_commit(
    wl_surface: *mut c_void,
    buffer: *mut c_void,
    width: u32,
    height: u32,
) {
    if wl_surface.is_null() {
        return;
    }
    let surface = wl_surface as *mut wl_proxy;

    // wl_surface.attach(new_buffer, x, y) — opcode 1
    let mut attach_args = [
        wl_argument { o: buffer },
        wl_argument { i: 0 },
        wl_argument { i: 0 },
    ];
    unsafe {
        wl_proxy_marshal_array(surface, 1, attach_args.as_mut_ptr());
    }

    // wl_surface.damage_buffer(x, y, width, height) — opcode 9
    // (buffer coordinates, not surface coordinates; correct with buffer_scale)
    let mut damage_args = [
        wl_argument { i: 0 },
        wl_argument { i: 0 },
        wl_argument { u: width },
        wl_argument { u: height },
    ];
    unsafe {
        wl_proxy_marshal_array(surface, 9, damage_args.as_mut_ptr());
    }

    // wl_surface.commit() — opcode 6
    let mut commit_args: [wl_argument; 0] = [];
    unsafe {
        wl_proxy_marshal_array(surface, 6, commit_args.as_mut_ptr());
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn flux_result_is_ok<T>(_r: T) -> bool {
    let v: i32 = unsafe { std::mem::transmute_copy(&std::mem::ManuallyDrop::new(_r)) };
    v == 0
}

fn flux_last_error_string(function: &str) -> String {
    let mut info: flux_error_info = unsafe { std::mem::zeroed() };
    unsafe { flux_get_last_error(&mut info) };
    let msg = if info.message.is_null() {
        "(no message)".to_string()
    } else {
        unsafe { std::ffi::CStr::from_ptr(info.message) }
            .to_string_lossy()
            .into_owned()
    };
    format!("{function} failed: {msg}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_number_label_matches_selection_keys() {
        let labels: Vec<_> = (0..10)
            .map(|i| String::from_utf8_lossy(&candidate_number_label(i)).into_owned())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]);
        assert_eq!(String::from_utf8_lossy(&candidate_number_label(10)), "11");
    }

    #[test]
    fn viewport_mapping_changes_when_scale_changes_but_logical_size_does_not() {
        assert!(viewport_mapping_changed(100, 40, 100, 40, 200, 80, 100, 40));
    }

    #[test]
    fn viewport_mapping_is_stable_when_source_and_destination_match() {
        assert!(!viewport_mapping_changed(
            200, 80, 100, 40, 200, 80, 100, 40
        ));
    }

    #[test]
    fn flux_last_error_string_is_readable() {
        let s = flux_last_error_string("test_function");
        assert!(s.contains("test_function"));
    }
}
