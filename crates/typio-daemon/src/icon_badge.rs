//! CPU rasteriser for language text badges into SNI ARGB32 pixmaps.
//!
//! Uses the shared flux-text and flux-canvas stack, the same text backend as
//! the candidate panel.

use flux_sys::{
    flux_canvas_cpu_begin, flux_canvas_cpu_end, flux_canvas_cpu_pixels, flux_canvas_create_cpu,
    flux_canvas_release,
};
use flux_text_sys::{
    flux_text_create, flux_text_desc, flux_text_draw, flux_text_family, flux_text_measure,
    flux_text_release, flux_text_style,
};

/// One rasterised badge bitmap at a single size. `argb` is `width*height`
/// pixels, 4 bytes each, big-endian ARGB32 (SNI byte order: `[A, R, G, B]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadgePixmap {
    pub width: i32,
    pub height: i32,
    pub argb: Vec<u8>,
}

fn color_rgba(rgb: u32, a: u8) -> u32 {
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    u32::from_le_bytes([r, g, b, a])
}

/// Owns the per-call `flux_text` context and releases it on every exit path.
///
/// A `flux_text` owns a 16 MiB host-coverage atlas plus the FreeType /
/// HarfBuzz / fontconfig backend, so a context that escapes `render` is a
/// multi-megabyte leak. `render`'s early returns (empty result when a size
/// produces no visible coverage) previously skipped `flux_text_release`
/// entirely; because an empty pixmap set also defeats `Tray::set_badge`'s
/// `had_badge` dedup, every tray refresh (each 中/EN mode toggle, engine or
/// language switch) re-ran `render` and leaked another context.
struct TextContext(*mut flux_text_sys::flux_text);

impl Drop for TextContext {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { flux_text_release(self.0) };
        }
    }
}

/// RAII guard for the per-size CPU canvas inside `render`.
struct CanvasGuard(*mut flux_sys::flux_canvas);

impl Drop for CanvasGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { flux_canvas_release(self.0) };
        }
    }
}

pub fn render(text: &str, sizes: &[u32], fg_rgb: u32) -> Vec<BadgePixmap> {
    if text.is_empty() || sizes.is_empty() {
        return Vec::new();
    }

    let desc = flux_text_desc {
        device: std::ptr::null_mut(),
        scale: 1.0,
    };
    let mut raw = std::ptr::null_mut();
    unsafe { flux_text_create(&desc, &mut raw) };
    if raw.is_null() {
        return Vec::new();
    }
    let text_ctx = TextContext(raw);

    let mut out = Vec::with_capacity(sizes.len());
    for &size in sizes {
        if size == 0 {
            continue;
        }

        let mut canvas = std::ptr::null_mut();
        unsafe { flux_canvas_create_cpu(size, size, 1.0, &mut canvas) };
        if canvas.is_null() {
            continue;
        }
        // Released by Drop on every path below, including the `vec![0u8; …]`
        // allocation failure path between create and the manual destroy.
        let _canvas_guard = CanvasGuard(canvas);

        let style = flux_text_style {
            size_px: size as f32 * 0.75, // Scale to fit
            weight: 0.0,
            color: color_rgba(fg_rgb, 255),
            family: flux_text_family::FLUX_TEXT_FAMILY_DEFAULT,
            italic: false,
        };
        let black_style = flux_text_style {
            color: color_rgba(0x000000, 255),
            ..style
        };

        unsafe {
            let m = flux_text_measure(text_ctx.0, text.as_ptr() as *const i8, text.len(), &style);

            // Center the text
            let x = (size as f32 - m.width) / 2.0;
            let y = (size as f32 - m.height) / 2.0;

            let _ = flux_canvas_cpu_begin(canvas, std::ptr::null());
            // Draw outline
            let offsets: [(f32, f32); 8] = [
                (-1.0, -1.0),
                (0.0, -1.0),
                (1.0, -1.0),
                (-1.0, 0.0),
                (1.0, 0.0),
                (-1.0, 1.0),
                (0.0, 1.0),
                (1.0, 1.0),
            ];
            for (ox, oy) in offsets {
                flux_text_draw(
                    text_ctx.0,
                    canvas,
                    std::ptr::null_mut(),
                    x + ox,
                    y + oy,
                    text.as_ptr() as *const i8,
                    text.len(),
                    &black_style,
                );
            }

            // Draw foreground
            flux_text_draw(
                text_ctx.0,
                canvas,
                std::ptr::null_mut(),
                x,
                y,
                text.as_ptr() as *const i8,
                text.len(),
                &style,
            );

            flux_canvas_cpu_end(canvas);

            let mut w = 0;
            let mut h = 0;
            let mut stride = 0;
            let px = flux_canvas_cpu_pixels(canvas, &mut w, &mut h, &mut stride);

            if !px.is_null() && w > 0 && h > 0 {
                // Convert from RGBA8 (premultiplied) to ARGB32 big-endian
                let len = (stride * h) as usize;
                let slice = std::slice::from_raw_parts(px, len);
                let mut argb = vec![0u8; (w * h * 4) as usize];
                let mut any_coverage = false;

                for row in 0..(h as usize) {
                    for col in 0..(w as usize) {
                        let src_idx = row * (stride as usize) + col * 4;
                        let dst_idx = (row * (w as usize) + col) * 4;
                        let r = slice[src_idx];
                        let g = slice[src_idx + 1];
                        let b = slice[src_idx + 2];
                        let a = slice[src_idx + 3];

                        if a > 0 {
                            any_coverage = true;
                        }

                        // SNI wants ARGB32 big endian ([A, R, G, B])
                        argb[dst_idx] = a;
                        argb[dst_idx + 1] = r;
                        argb[dst_idx + 2] = g;
                        argb[dst_idx + 3] = b;
                    }
                }

                if any_coverage {
                    out.push(BadgePixmap {
                        width: w as i32,
                        height: h as i32,
                        argb,
                    });
                }
            }
        }
    }

    if out.len() != sizes.len() {
        // Partial coverage: the caller treats an incomplete ladder as "no
        // badge" (icon fallback). The TextContext guard above releases the
        // flux-text context on this early return — this path used to leak it.
        return Vec::new();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_coverage(p: &BadgePixmap) -> bool {
        p.argb.chunks_exact(4).any(|px| px[0] != 0)
    }

    #[test]
    fn rejects_bad_input() {
        assert!(render("", &[22], 0xFFFFFF).is_empty());
    }

    #[test]
    fn partial_ladder_releases_flux_text_context() {
        // A size-0 entry produces no pixmap, so `out.len() != sizes.len()`
        // and render returns early via the partial-coverage path. That path
        // used to skip `flux_text_release`, leaking the flux-text context
        // (16 MiB host-coverage atlas + FreeType/HarfBuzz/fontconfig backend)
        // on every tray refresh while no badge pixmap dedup was active.
        // Run it repeatedly: under valgrind/LSAN the old code reports a
        // definite leak that grows linearly with iterations.
        for _ in 0..8 {
            assert!(render("EN", &[0, 22, 0], 0xFFFFFF).is_empty());
        }
    }

    #[test]
    fn renders_latin_badge() {
        let sizes = [22u32, 44];
        let out = render("EN", &sizes, 0xFFFFFF);
        if out.is_empty() {
            eprintln!("no covering font for Latin in this env — skipping");
            return;
        }
        assert_eq!(out.len(), 2);
        for (i, p) in out.iter().enumerate() {
            assert_eq!(p.width, sizes[i] as i32);
            assert_eq!(p.height, sizes[i] as i32);
            assert_eq!(p.argb.len(), (sizes[i] * sizes[i] * 4) as usize);
            assert!(has_coverage(p), "size {} produced no coverage", sizes[i]);
        }
    }

    #[test]
    fn renders_cjk_badge_when_font_present() {
        let out = render("中", &[44], 0xFFFFFF);
        if out.is_empty() {
            eprintln!("no covering CJK font in this env — skipping");
            return;
        }
        assert_eq!(out.len(), 1);
        assert!(has_coverage(&out[0]));
    }
}
