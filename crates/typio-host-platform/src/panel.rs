//! Candidate panel — flux CPU (software) rendering + host-managed SHM buffer.
//!
//! flux's CPU canvas (`flux_canvas_create_cpu`) rasterises the panel background,
//! selection highlight and text on the host — no Vulkan device, surface,
//! swapchain or dma-buf. Text is shaped by flux-text and drawn into the same
//! canvas pass via the host-coverage glyph path (ADR-0019). The composited
//! RGBA8 framebuffer flux returns from `flux_canvas_cpu_pixels` is byte-swapped
//! into a host-owned `wl_shm` `wl_buffer` (RGBA→ARGB8888) and attached to the
//! popup `wl_surface`.
//!
//! ```text
//! flux CPU canvas (bg + highlight + text)  →  flux_canvas_cpu_pixels (RGBA8 premul)
//!   → present_shm: RGBA→ARGB8888 (byte-swap) straight into a free ShmBuffer
//!     → wl_surface.attach + damage + commit (raw FFI, returns instantly)
//! ```

use std::collections::hash_map::DefaultHasher;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::ptr;
use std::time::Instant;

use flux_sys::{
    flux_canvas, flux_canvas_cpu_begin, flux_canvas_cpu_end, flux_canvas_cpu_pixels,
    flux_canvas_create_cpu, flux_canvas_destroy, flux_canvas_fill_rrect, flux_color_rgba,
    flux_error_info, flux_get_last_error, flux_rect,
};
use wayland_client::protocol::wl_shm;
use wayland_client::{Proxy, QueueHandle};
use wayland_sys::{
    client::{wl_proxy, wl_proxy_marshal_array},
    common::wl_argument,
};

use crate::PanelFontConfig;
use crate::protocols::viewporter::wp_viewport::WpViewport;
use crate::text_raster::{TextMetrics, TextRaster};

/// Offscreen width quantum (grow-only; cropped to exact content via wp_viewport).
const SURFACE_WIDTH_QUANTUM: u32 = 64;
/// Height quantum (same grow-only logic).
const SURFACE_HEIGHT_QUANTUM: u32 = 32;
/// Initial framebuffer size, sized to skip the first banner's resize.
pub const PANEL_PREALLOC_WIDTH: u32 = 512;
pub const PANEL_PREALLOC_HEIGHT: u32 = 128;

const PANEL_PADDING: f32 = 8.0;
const PANEL_ROW_HEIGHT: f32 = 24.0;
/// Lower bound for the candidate-row height fold when no text is measured.
/// Kept as the legacy default size; the actual draw size comes from the
/// panel font config.
const CANDIDATE_FONT_SIZE: f32 = 16.0;
const CANDIDATE_ITEM_X_PADDING: f32 = 5.0;
const CANDIDATE_ITEM_GAP: f32 = 8.0;
const CANDIDATE_NUMBER_GAP: f32 = 4.0;

const BANNER_PADDING: f32 = 10.0;

const TEXT_COLOR: [u8; 3] = [240, 240, 240];
const NUMBER_COLOR: [u8; 3] = [145, 145, 152];
const CANDIDATE_ROW_EXTRA_LEADING: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutCacheKey {
    scale_bits: u32,
    candidate_count: usize,
    content_hash: u64,
}

impl LayoutCacheKey {
    fn new(candidates: &[String], scale: f32) -> Self {
        let mut hasher = DefaultHasher::new();
        for candidate in candidates {
            candidate.hash(&mut hasher);
        }
        Self {
            scale_bits: scale.to_bits(),
            candidate_count: candidates.len(),
            content_hash: hasher.finish(),
        }
    }
}

/// A candidate panel backed by flux software rendering and Wayland SHM.
pub struct FluxPanel {
    canvas: *mut flux_canvas,
    canvas_w: u32,
    canvas_h: u32,
    canvas_scale: f32,
    text: TextRaster,
    width: u32,
    height: u32,
    scale: f32,
    /// Panel font configuration (family + size). Drives the candidate /
    /// number / banner sizes and the [`TextRaster`] primary family.
    font: PanelFontConfig,
    viewport: Option<WpViewport>,
    viewport_source_w_physical: u32,
    viewport_source_h_physical: u32,
    content_w_logical: i32,
    content_h_logical: i32,
    wl_surface: *mut c_void,
    shm_pool: Option<crate::panel_shm::ShmBufferPool>,
    last_layout_key: Option<LayoutCacheKey>,
    last_layout: Vec<(TextMetrics, TextMetrics)>,
}

impl FluxPanel {
    /// Create a panel that renders on the CPU via flux and presents through a
    /// host-managed `wl_shm` buffer on `wl_surface`.
    ///
    /// # Safety
    /// `wl_surface_ptr` must be a valid `*mut wl_surface` for the panel's lifetime.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new_from_surface(
        wl_surface_ptr: *mut c_void,
        viewport: Option<WpViewport>,
        shm: Option<wl_shm::WlShm>,
        qh: QueueHandle<crate::input_method::InputMethodState>,
        registry: crate::panel_shm::ShmReleaseRegistry,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        if wl_surface_ptr.is_null() {
            return Err("wl_surface is null".into());
        }

        let mut canvas: *mut flux_canvas = ptr::null_mut();
        let r = flux_canvas_create_cpu(width.max(1), height.max(1), 1.0, &mut canvas);
        if !flux_result_is_ok(r) || canvas.is_null() {
            return Err(flux_last_error_string("flux_canvas_create_cpu"));
        }

        Ok(Self {
            canvas,
            canvas_w: width,
            canvas_h: height,
            canvas_scale: 1.0,
            text: TextRaster::default(),
            width,
            height,
            scale: 1.0,
            font: PanelFontConfig::default(),
            viewport,
            viewport_source_w_physical: 0,
            viewport_source_h_physical: 0,
            content_w_logical: 0,
            content_h_logical: 0,
            wl_surface: wl_surface_ptr,
            shm_pool: shm
                .as_ref()
                .map(|s| crate::panel_shm::ShmBufferPool::new(s.clone(), qh.clone(), registry)),
            last_layout_key: None,
            last_layout: Vec::new(),
        })
    }

    pub fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() < 0.01 {
            return;
        }
        self.scale = scale;
        self.invalidate_layout_cache();
    }

    /// Apply the current panel font configuration (family + size). When the
    /// family changes, the [`TextRaster`] flushes its per-codepoint and per-face
    /// caches; either way the layout cache is dropped so candidate/banner
    /// geometry is re-measured at the new size.
    pub fn set_font_config(&mut self, cfg: PanelFontConfig) {
        if self.font == cfg {
            return;
        }
        let family_changed = self.font.family != cfg.family;
        self.font = cfg;
        if family_changed {
            self.text.set_preferred_family(self.font.family_opt());
        }
        self.invalidate_layout_cache();
    }

    /// Drop the cached per-candidate layout, forcing the next draw/size call to
    /// re-measure.
    pub fn invalidate_layout_cache(&mut self) {
        self.last_layout_key = None;
        self.last_layout.clear();
    }

    /// (Re)create the CPU canvas when framebuffer size or scale changed.
    fn ensure_canvas(&mut self) -> bool {
        if !self.canvas.is_null()
            && self.canvas_w == self.width
            && self.canvas_h == self.height
            && (self.canvas_scale - self.scale).abs() < 0.001
        {
            return true;
        }
        unsafe {
            if !self.canvas.is_null() {
                flux_canvas_destroy(self.canvas);
                self.canvas = ptr::null_mut();
            }
            let mut canvas: *mut flux_canvas = ptr::null_mut();
            let r = flux_canvas_create_cpu(
                self.width.max(1),
                self.height.max(1),
                self.scale.max(0.1),
                &mut canvas,
            );
            if !flux_result_is_ok(r) || canvas.is_null() {
                tracing::warn!(
                    target: "typio.panel.cpu",
                    "flux_canvas_create_cpu failed on resize"
                );
                return false;
            }
            self.canvas = canvas;
            self.canvas_w = self.width;
            self.canvas_h = self.height;
            self.canvas_scale = self.scale;
        }
        true
    }

    /// Draw the opaque rounded-rect panel body over the content rect (logical).
    unsafe fn draw_panel_background(&mut self) {
        let w = self.content_w_logical as f32 - 2.0;
        let h = self.content_h_logical as f32 - 2.0;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let rect = flux_rect {
            x: 1.0,
            y: 1.0,
            w,
            h,
        };
        let bg = flux_color_rgba(28, 28, 32, 255);
        flux_canvas_fill_rrect(self.canvas, rect, 8.0, bg);
    }

    /// Draw candidate strings with the selected one highlighted. Returns `true`
    /// iff a frame was actually attached to the surface.
    pub fn draw_candidates(
        &mut self,
        candidates: &[String],
        selected: usize,
        composition_seq: u64,
    ) -> bool {
        let trace_perf = tracing::enabled!(target: "typio.panel.perf", tracing::Level::TRACE);
        let frame_start = trace_perf.then(Instant::now);

        let layout_start = trace_perf.then(Instant::now);
        self.ensure_candidate_layout(candidates);
        let layout_us = layout_start.map(|t| t.elapsed().as_micros()).unwrap_or(0);

        if !self.ensure_canvas() {
            if trace_perf {
                tracing::trace!(
                    target: "typio.panel.perf",
                    composition_seq,
                    candidate_count = candidates.len(),
                    selected,
                    reason = "ensure_canvas_failed",
                    "candidate panel frame dropped before draw"
                );
            }
            return false;
        }

        let draw_start = trace_perf.then(Instant::now);

        // ── flux CPU pass: background + highlight + text (logical coords) ──
        // Text draws inside the pass via flux-text's host-coverage path
        // (ADR-0019); the canvas content-scale transform maps logical quads
        // onto physical pixels, so text uses logical coords like the fills.
        unsafe {
            if !flux_result_is_ok(flux_canvas_cpu_begin(self.canvas, ptr::null())) {
                return false;
            }
            self.draw_panel_background();
            let mut cx = PANEL_PADDING;
            let y = PANEL_PADDING;
            let row_height = candidate_row_height(&self.last_layout);
            for (i, _) in candidates.iter().enumerate() {
                let (num_m, m) = self.last_layout[i];
                let iw =
                    CANDIDATE_ITEM_X_PADDING * 2.0 + num_m.width + CANDIDATE_NUMBER_GAP + m.width;
                if i == selected {
                    let hi = flux_color_rgba(56, 84, 160, 255);
                    flux_canvas_fill_rrect(
                        self.canvas,
                        flux_rect {
                            x: cx,
                            y,
                            w: iw,
                            h: row_height,
                        },
                        4.0,
                        hi,
                    );
                }
                cx += iw + CANDIDATE_ITEM_GAP;
            }

            // Text pass (logical coords).
            let mut cx = PANEL_PADDING;
            let y = PANEL_PADDING;
            let row_height = candidate_row_height(&self.last_layout);
            for (i, candidate) in candidates.iter().enumerate() {
                let (num_m, m) = self.last_layout[i];
                let iw =
                    CANDIDATE_ITEM_X_PADDING * 2.0 + num_m.width + CANDIDATE_NUMBER_GAP + m.width;
                let text_top = y + (row_height - m.height).max(0.0) / 2.0;
                let number_top = text_top + m.baseline - num_m.baseline;
                let number_x = cx + CANDIDATE_ITEM_X_PADDING;
                let main_x = cx + CANDIDATE_ITEM_X_PADDING + num_m.width + CANDIDATE_NUMBER_GAP;

                let label = candidate_number_label(i);
                let number_str = std::str::from_utf8(&label).unwrap_or("");
                self.text.draw(
                    self.canvas,
                    number_x,
                    number_top,
                    number_str,
                    self.font.number_size_px(),
                    NUMBER_COLOR,
                );
                self.text.draw(
                    self.canvas,
                    main_x,
                    text_top,
                    candidate,
                    self.font.candidate_size_px(),
                    TEXT_COLOR,
                );
                cx += iw + CANDIDATE_ITEM_GAP;
            }
            flux_canvas_cpu_end(self.canvas);
        }
        let draw_us = draw_start.map(|t| t.elapsed().as_micros()).unwrap_or(0);

        // Background + text are both baked into the canvas framebuffer now;
        // present_shm reads it straight out and byte-swaps into the SHM buffer.
        let present_start = trace_perf.then(Instant::now);
        let attached = self.present_shm();
        let present_us = present_start.map(|t| t.elapsed().as_micros()).unwrap_or(0);

        if trace_perf {
            tracing::trace!(
                target: "typio.panel.perf",
                composition_seq,
                candidate_count = candidates.len(),
                selected,
                width = self.width,
                height = self.height,
                scale = self.scale,
                layout_us,
                draw_us,
                present_us,
                total_us = frame_start.map(|t| t.elapsed().as_micros()).unwrap_or(0),
                attached,
                "candidate panel frame"
            );
        }

        attached
    }

    /// Resize the framebuffer. The CPU canvas is recreated lazily on next draw.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
    }

    /// Cached `(number_metrics, text_metrics)` per candidate (logical px),
    /// re-measured only when the candidate strings or scale change.
    fn ensure_candidate_layout(&mut self, candidates: &[String]) {
        let key = LayoutCacheKey::new(candidates, self.scale);
        if self.last_layout_key == Some(key) {
            return;
        }

        self.last_layout.clear();
        self.last_layout.reserve(candidates.len());
        for (i, candidate) in candidates.iter().enumerate() {
            let label = candidate_number_label(i);
            let number_str = std::str::from_utf8(&label).unwrap_or("");
            let num_m = self.text.measure(number_str, self.font.number_size_px());
            let m = self.text.measure(candidate, self.font.candidate_size_px());
            self.last_layout.push((num_m, m));
        }
        self.last_layout_key = Some(key);
    }

    /// Ensure the framebuffer is big enough for the candidate row.
    pub fn ensure_candidate_size(&mut self, candidates: &[String]) {
        self.ensure_candidate_layout(candidates);
        let layout = &self.last_layout;
        let mut total_width: f32 = PANEL_PADDING;
        for (num_m, m) in layout.iter() {
            let iw = CANDIDATE_ITEM_X_PADDING * 2.0 + num_m.width + CANDIDATE_NUMBER_GAP + m.width;
            total_width += iw + CANDIDATE_ITEM_GAP;
        }
        if !candidates.is_empty() {
            total_width = total_width - CANDIDATE_ITEM_GAP + PANEL_PADDING;
        } else {
            total_width += PANEL_PADDING;
        }

        let desired_width = (total_width as u32).max(10);
        let desired_height = (PANEL_PADDING * 2.0 + candidate_row_height(&layout)).ceil() as u32;
        let phys_width = (desired_width as f32 * self.scale).ceil() as u32;
        let phys_height = (desired_height as f32 * self.scale).ceil() as u32;
        self.apply_grow_only_size(
            phys_width,
            phys_height,
            desired_width as i32,
            desired_height as i32,
        );
    }

    /// Grow-only framebuffer sizing shared by candidate and banner paths.
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
            }
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
            let resized = self.width != phys_width || self.height != phys_height;
            self.width = phys_width;
            self.height = phys_height;
            self.content_w_logical = content_w_logical;
            self.content_h_logical = content_h_logical;
            resized
        }
    }

    /// Hide the panel by detaching its buffer.
    pub fn hide(&mut self) {
        unsafe {
            wl_surface_detach_and_commit(self.wl_surface);
        }
    }

    /// Present the flux CPU framebuffer (premultiplied RGBA8) via a host-managed
    /// SHM buffer, byte-swapping RGBA→ARGB8888 directly into the SHM pixels.
    /// Returns `true` iff a buffer was attached.
    fn present_shm(&mut self) -> bool {
        let trace_perf = tracing::enabled!(target: "typio.panel.perf", tracing::Level::TRACE);
        let present_start = trace_perf.then(Instant::now);
        let (width, height) = (self.width, self.height);
        let vp = self.viewport.is_some();
        let scale = self.scale;
        let ws = self.wl_surface;
        let canvas = self.canvas;

        let Some(pool) = self.shm_pool.as_mut() else {
            return false;
        };
        let acquire_start = trace_perf.then(Instant::now);
        let Some(idx) = pool.acquire(width, height) else {
            tracing::debug!(
                target: "typio.panel.shm",
                "shm buffer pool exhausted — dropping frame (will retry)"
            );
            return false;
        };
        let acquire_us = acquire_start.map(|t| t.elapsed().as_micros()).unwrap_or(0);
        let buf = pool.get(idx).expect("acquire returned a valid index");
        let dst = buf.pixels();
        let dst_len = buf.pixel_len();

        let copy_start = trace_perf.then(Instant::now);
        unsafe {
            // Read the flux canvas framebuffer straight off the canvas and
            // byte-swap premultiplied RGBA8 → Wayland ARGB8888 (LE B,G,R,A)
            // in a single pass into the SHM backing store — no intermediate copy.
            let (mut w, mut h, mut stride) = (0u32, 0u32, 0u32);
            let px = flux_canvas_cpu_pixels(canvas, &mut w, &mut h, &mut stride);
            if px.is_null() {
                return false;
            }
            let src_len = (h as usize) * (stride as usize);
            let src = std::slice::from_raw_parts(px, src_len);
            let dstm = std::slice::from_raw_parts_mut(dst, dst_len);
            let n = src_len.min(dst_len);
            let mut i = 0;
            while i + 4 <= n {
                dstm[i] = src[i + 2]; // B
                dstm[i + 1] = src[i + 1]; // G
                dstm[i + 2] = src[i]; // R
                dstm[i + 3] = src[i + 3]; // A
                i += 4;
            }
        }
        let copy_us = copy_start.map(|t| t.elapsed().as_micros()).unwrap_or(0);
        buf.mark_busy();

        let attach_start = trace_perf.then(Instant::now);
        let desired_scale = if vp { 1 } else { scale as i32 };
        unsafe {
            wl_surface_set_buffer_scale(ws, desired_scale);
            wl_surface_attach_commit(
                ws,
                buf.wl_buffer().id().as_ptr() as *mut c_void,
                width,
                height,
            );
        }
        let attach_us = attach_start.map(|t| t.elapsed().as_micros()).unwrap_or(0);

        if trace_perf {
            tracing::trace!(
                target: "typio.panel.perf",
                width,
                height,
                scale,
                buffer_index = idx,
                acquire_us,
                copy_us,
                attach_us,
                total_us = present_start.map(|t| t.elapsed().as_micros()).unwrap_or(0),
                "candidate panel present_shm"
            );
        }

        true
    }

    /// Draw a single centred status banner (indicator / voice). Returns `true`
    /// iff a frame was attached. Empty labels are ignored (`false`).
    pub fn draw_status_banner(&mut self, label: &str) -> bool {
        if label.is_empty() {
            return false;
        }
        let m = self.text.measure(label, self.font.banner_size_px());
        if !self.ensure_canvas() {
            return false;
        }
        unsafe {
            if !flux_result_is_ok(flux_canvas_cpu_begin(self.canvas, ptr::null())) {
                return false;
            }
            self.draw_panel_background();

            // Text draws inside the pass (host-coverage path, logical coords).
            let text_y =
                BANNER_PADDING + (self.font.banner_size_px() * 1.3 - m.height).max(0.0) / 2.0;
            self.text.draw(
                self.canvas,
                BANNER_PADDING,
                text_y,
                label,
                self.font.banner_size_px(),
                TEXT_COLOR,
            );

            flux_canvas_cpu_end(self.canvas);
        }

        self.present_shm()
    }

    /// Ensure the framebuffer fits a single-row banner of `label`.
    pub fn ensure_banner_size(&mut self, label: &str) {
        let m = self.text.measure(label, self.font.banner_size_px());
        let desired_width = (BANNER_PADDING * 2.0 + m.width).max(10.0).ceil() as u32;
        let banner_row_height = BANNER_PADDING * 2.0 + self.font.banner_size_px() * 1.3;
        let desired_height = banner_row_height.ceil() as u32;
        let phys_width = (desired_width as f32 * self.scale).ceil() as u32;
        let phys_height = (desired_height as f32 * self.scale).ceil() as u32;
        self.apply_grow_only_size(
            phys_width,
            phys_height,
            desired_width as i32,
            desired_height as i32,
        );
    }
}

#[allow(clippy::too_many_arguments)]
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
            if !self.canvas.is_null() {
                flux_canvas_destroy(self.canvas);
                self.canvas = ptr::null_mut();
            }
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

fn candidate_row_height(layout: &[(TextMetrics, TextMetrics)]) -> f32 {
    let measured = layout
        .iter()
        .fold(CANDIDATE_FONT_SIZE, |height, (num, text)| {
            height.max(num.height).max(text.height)
        });
    PANEL_ROW_HEIGHT.max((measured + CANDIDATE_ROW_EXTRA_LEADING).ceil())
}

// ── Raw Wayland surface requests via wayland-sys ──────────────────────────

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
    wl_proxy_marshal_array(surface, 1, attach_args.as_mut_ptr());
    let mut commit_args: [wl_argument; 0] = [];
    wl_proxy_marshal_array(surface, 6, commit_args.as_mut_ptr());
}

unsafe fn wl_surface_set_buffer_scale(wl_surface: *mut c_void, scale: i32) {
    if wl_surface.is_null() {
        return;
    }
    let surface = wl_surface as *mut wl_proxy;
    let mut args = [wl_argument { i: scale }];
    wl_proxy_marshal_array(surface, 8, args.as_mut_ptr());
}

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
    let mut attach_args = [
        wl_argument { o: buffer },
        wl_argument { i: 0 },
        wl_argument { i: 0 },
    ];
    wl_proxy_marshal_array(surface, 1, attach_args.as_mut_ptr());
    let mut damage_args = [
        wl_argument { i: 0 },
        wl_argument { i: 0 },
        wl_argument { u: width },
        wl_argument { u: height },
    ];
    wl_proxy_marshal_array(surface, 9, damage_args.as_mut_ptr());
    let mut commit_args: [wl_argument; 0] = [];
    wl_proxy_marshal_array(surface, 6, commit_args.as_mut_ptr());
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
    fn layout_cache_key_tracks_content_without_cloning_candidates() {
        let a = vec!["候选".to_string(), "candidate".to_string()];
        let b = vec!["候选".to_string(), "candidate".to_string()];
        let c = vec!["候选".to_string(), "different".to_string()];

        assert_eq!(LayoutCacheKey::new(&a, 1.0), LayoutCacheKey::new(&b, 1.0));
        assert_ne!(LayoutCacheKey::new(&a, 1.0), LayoutCacheKey::new(&c, 1.0));
        assert_ne!(LayoutCacheKey::new(&a, 1.0), LayoutCacheKey::new(&a, 2.0));
    }

    #[test]
    fn candidate_row_height_expands_for_tall_fallback_metrics() {
        let layout = [(
            TextMetrics {
                width: 6.0,
                height: 10.0,
                baseline: 8.0,
            },
            TextMetrics {
                width: 18.0,
                height: 31.0,
                baseline: 24.0,
            },
        )];

        assert_eq!(candidate_row_height(&[]), PANEL_ROW_HEIGHT);
        assert_eq!(candidate_row_height(&layout), 35.0);
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
