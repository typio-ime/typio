//! CPU text rasteriser for the candidate panel and status banners.
//!
//! flux's CPU canvas backend (`flux_canvas_create_cpu`) rasterises fills,
//! gradients, rounded-rects and paths on the host, but *silently drops* image
//! and glyph draws — they need GPU-resident textures. The panel therefore
//! shapes and rasterises its own text here and composites it straight into the
//! premultiplied-RGBA8 framebuffer flux hands back from
//! `flux_canvas_cpu_pixels`.
//!
//! Stack (same pure-Rust trio as `icon_badge`): `rustybuzz` (shaping + script
//! itemisation), `fontdb` (per-codepoint face fallback) and `ab_glyph`
//! (outline rasterisation). All public sizes/positions are in physical pixels;
//! [`TextRaster::measure`] reports logical-pixel extents (scale-independent) so
//! the panel's logical layout maths is unchanged.

use std::collections::HashMap;
use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, Glyph, GlyphId, PxScale};
use fontdb::{Database, FaceInfo, Style, ID};
use rustybuzz::{Face as RbFace, UnicodeBuffer};

/// Shaped extent of a run in logical pixels. Mirrors the fields the panel
/// layout previously consumed from `flux_text_metrics`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

/// Process-lifetime system font database (loaded once).
fn font_db() -> &'static Database {
    static DB: OnceLock<Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        db
    })
}

struct FontEntry {
    /// Raw font-file bytes, kept for on-demand `rustybuzz::Face` creation.
    data: Vec<u8>,
    index: u32,
    /// ab_glyph outline source (owns its own copy of the bytes).
    font: FontVec,
}

const CJK_SANS_FAMILIES: &[&str] = &[
    "Noto Sans SC",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
    "WenQuanYi Micro Hei",
    "Microsoft YaHei",
    "PingFang SC",
    "Noto Sans TC",
    "Noto Sans CJK TC",
    "Noto Sans HK",
    "Noto Sans CJK HK",
    "Noto Sans JP",
    "Noto Sans CJK JP",
    "Noto Sans KR",
    "Noto Sans CJK KR",
    "Apple SD Gothic Neo",
    "Malgun Gothic",
];

const UI_SANS_FAMILIES: &[&str] = &[
    "Noto Sans",
    "Inter",
    "Roboto",
    "DejaVu Sans",
    "Liberation Sans",
    "Arial",
];

fn is_cjk_candidate_char(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x2EFF
            | 0x2F00..=0x2FDF
            | 0x3000..=0x303F
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0x3100..=0x312F
            | 0x3130..=0x318F
            | 0x31A0..=0x31BF
            | 0x31C0..=0x31EF
            | 0x3200..=0x32FF
            | 0x3300..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
            | 0x30000..=0x3134F
    )
}

fn family_rank(face: &FaceInfo, preferred: &[&str]) -> Option<usize> {
    face.families.iter().find_map(|(family, _)| {
        preferred
            .iter()
            .position(|preferred| family.eq_ignore_ascii_case(preferred))
    })
}

/// Whether `face` belongs to the user-configured primary family. This is the
/// highest-priority tier in [`face_score`]: when the primary family covers the
/// codepoint it always wins over the built-in fallback lists, giving the panel
/// a "primary font then fallback" semantics identical to fontconfig.
fn family_is_primary(face: &FaceInfo, primary: Option<&str>) -> bool {
    match primary {
        Some(p) if !p.is_empty() => face
            .families
            .iter()
            .any(|(family, _)| family.eq_ignore_ascii_case(p)),
        _ => false,
    }
}

fn family_contains(face: &FaceInfo, needle: &str) -> bool {
    face.families
        .iter()
        .any(|(family, _)| family.to_ascii_lowercase().contains(needle))
}

fn face_score(face: &FaceInfo, c: char, primary: Option<&str>) -> (usize, u8, u16) {
    // Tier 0: the user-configured primary family always wins when it covers the
    // codepoint. This is the fontconfig "first font" — everything else is a
    // tofu-avoidance fallback.
    let script_score = if family_is_primary(face, primary) {
        0
    } else if is_cjk_candidate_char(c) {
        if let Some(rank) = family_rank(face, CJK_SANS_FAMILIES) {
            rank
        } else if family_contains(face, "sans cjk") || family_contains(face, "sans sc") {
            100
        } else if family_contains(face, "serif") {
            900
        } else if family_contains(face, "emoji") || family_contains(face, "jigmo") {
            1000
        } else if face.monospaced {
            700
        } else {
            500
        }
    } else if let Some(rank) = family_rank(face, UI_SANS_FAMILIES) {
        rank
    } else if family_rank(face, CJK_SANS_FAMILIES).is_some() {
        250
    } else if family_contains(face, "serif") {
        900
    } else if family_contains(face, "emoji") {
        1000
    } else if face.monospaced {
        700
    } else {
        500
    };
    // Prefer upright faces: an Italic/Oblique face must never win a tie over
    // the Normal member of the same family (default text is not slanted).
    let style_penalty = if face.style == Style::Normal { 0 } else { 1 };
    // CJK 在小字号下 Regular(400) 笔画偏细，与拉丁文视觉不平衡；优先 Medium(500)，
    // 系统若无 Medium 会自然回落到最接近的字重。拉丁/UI 文字仍以 400 为准。
    let target_weight = if is_cjk_candidate_char(c) { 500 } else { 400 };
    let weight_delta = face.weight.0.abs_diff(target_weight);
    (script_score, style_penalty, weight_delta)
}

fn face_covers(db: &Database, id: ID, c: char) -> bool {
    db.with_face_data(id, |data, index| {
        rustybuzz::ttf_parser::Face::parse(data, index)
            .ok()
            .and_then(|f| f.glyph_index(c))
            .is_some()
    })
    .unwrap_or(false)
}

/// Shaping + rasterisation front-end with per-face and per-codepoint caches.
pub struct TextRaster {
    entries: Vec<FontEntry>,
    by_face: HashMap<ID, usize>,
    cover: HashMap<char, Option<usize>>,
    /// User-configured primary font family (the fontconfig "first font").
    /// When `Some`, faces of this family always outrank the built-in fallback
    /// lists. Empty/`None` ⇒ pure fallback selection.
    preferred_family: Option<String>,
}

impl Default for TextRaster {
    fn default() -> Self {
        Self::new(None)
    }
}

impl TextRaster {
    pub fn new(preferred_family: Option<String>) -> Self {
        Self {
            entries: Vec::new(),
            by_face: HashMap::new(),
            cover: HashMap::new(),
            preferred_family: preferred_family.filter(|s| !s.is_empty()),
        }
    }

    /// Change the primary font family at runtime (config reload). Flushes the
    /// per-codepoint coverage cache and all loaded faces, since a different
    /// primary can resolve a codepoint to a different file.
    pub fn set_preferred_family(&mut self, family: Option<String>) {
        let family = family.filter(|s| !s.is_empty());
        if self.preferred_family == family {
            return;
        }
        self.preferred_family = family;
        self.entries.clear();
        self.by_face.clear();
        self.cover.clear();
    }

    /// Index into `self.entries` of the first face covering `c`, loading it on
    /// first use. `None` when no system face covers `c`.
    fn entry_for_char(&mut self, c: char) -> Option<usize> {
        if let Some(&hit) = self.cover.get(&c) {
            return hit;
        }
        let db = font_db();
        let primary = self.preferred_family.as_deref();
        let found = db
            .faces()
            .filter(|face| face_covers(db, face.id, c))
            .min_by_key(|face| face_score(face, c, primary))
            .and_then(|face| self.load_entry(db, face.id));
        self.cover.insert(c, found);
        found
    }

    fn load_entry(&mut self, db: &Database, id: ID) -> Option<usize> {
        if let Some(&i) = self.by_face.get(&id) {
            return Some(i);
        }
        let (bytes, index) = db.with_face_data(id, |data, index| (data.to_vec(), index))?;
        let font = FontVec::try_from_vec_and_index(bytes.clone(), index).ok()?;
        self.entries.push(FontEntry {
            data: bytes,
            index,
            font,
        });
        let i = self.entries.len() - 1;
        self.by_face.insert(id, i);
        Some(i)
    }

    /// Split `text` into maximal runs sharing one covering face.
    fn runs(&mut self, text: &str) -> Vec<(usize, String)> {
        let mut runs: Vec<(usize, String)> = Vec::new();
        for c in text.chars() {
            let Some(idx) = self.entry_for_char(c) else {
                continue; // uncovered codepoint — skip (tofu avoided)
            };
            match runs.last_mut() {
                Some((fi, s)) if *fi == idx => s.push(c),
                _ => runs.push((idx, c.to_string())),
            }
        }
        runs
    }

    /// Measure `text` at `size_px` (logical). Returns width plus baseline/height
    /// derived from the covering faces.
    pub fn measure(&mut self, text: &str, size_px: f32) -> TextMetrics {
        if text.is_empty() || size_px <= 0.0 {
            return TextMetrics::default();
        }
        let runs = self.runs(text);
        let mut width = 0.0f32;
        let mut ascent = 0.0f32;
        let mut descent = 0.0f32;
        for (fi, run) in &runs {
            let entry = &self.entries[*fi];
            let Some(face) = RbFace::from_slice(&entry.data, entry.index) else {
                continue;
            };
            let upem = face.units_per_em() as f32;
            if upem <= 0.0 {
                continue;
            }
            let sf = size_px / upem;
            ascent = ascent.max(face.ascender() as f32 * sf);
            descent = descent.max(-(face.descender() as f32) * sf);
            let mut buf = UnicodeBuffer::new();
            buf.push_str(run);
            buf.guess_segment_properties();
            let shaped = rustybuzz::shape(&face, &[], buf);
            for pos in shaped.glyph_positions() {
                width += pos.x_advance as f32 * sf;
            }
        }
        let height = ascent + descent;
        TextMetrics {
            width,
            height: if height > 0.0 { height } else { size_px },
            baseline: if ascent > 0.0 { ascent } else { size_px * 0.8 },
        }
    }

    /// Composite `text` into `buf` (premultiplied RGBA8, `buf_w`×`buf_h`) with
    /// the text box top-left at (`x`, `y`) physical pixels, at `size_px`
    /// physical, in solid `color` (straight RGB; per-pixel coverage → alpha).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        buf: &mut [u8],
        buf_w: u32,
        buf_h: u32,
        x: f32,
        y: f32,
        text: &str,
        size_px: f32,
        color: [u8; 3],
    ) {
        if text.is_empty() || size_px <= 0.0 {
            return;
        }
        let runs = self.runs(text);

        // Common baseline = tallest ascent across runs.
        let mut ascent = 0.0f32;
        for (fi, _) in &runs {
            let entry = &self.entries[*fi];
            if let Some(face) = RbFace::from_slice(&entry.data, entry.index) {
                let upem = face.units_per_em() as f32;
                if upem > 0.0 {
                    ascent = ascent.max(face.ascender() as f32 * (size_px / upem));
                }
            }
        }
        if ascent <= 0.0 {
            ascent = size_px * 0.8;
        }
        let baseline_y = y + ascent;
        let mut pen_x = x;

        for (fi, run) in &runs {
            let entry = &self.entries[*fi];
            let font = &entry.font;
            let Some(face) = RbFace::from_slice(&entry.data, entry.index) else {
                continue;
            };
            let upem = face.units_per_em() as f32;
            if upem <= 0.0 {
                continue;
            }
            let sf = size_px / upem;

            // ab_glyph 会用 height_unscaled(ascender-descender) 而非 units_per_em
            // 来缩放轮廓，导致每种字体的实际字号被乘上 upem/(asc-desc)。
            // 这里按 face 补偿，使光栅化的实际倍率与下面 advance 用的 sf 一致，
            // 否则主字体(拉丁)会偏小、CJK 兜底字体却正常。
            let px_scale = PxScale::from(size_px * font.height_unscaled() / upem);

            let mut ub = UnicodeBuffer::new();
            ub.push_str(run);
            ub.guess_segment_properties();
            let shaped = rustybuzz::shape(&face, &[], ub);
            let infos = shaped.glyph_infos();
            let positions = shaped.glyph_positions();

            for (info, pos) in infos.iter().zip(positions.iter()) {
                let gx = pen_x + pos.x_offset as f32 * sf;
                let gy = baseline_y - pos.y_offset as f32 * sf;
                pen_x += pos.x_advance as f32 * sf;

                let glyph = Glyph {
                    id: GlyphId(info.glyph_id as u16),
                    scale: px_scale,
                    position: ab_glyph::point(gx, gy),
                };
                let Some(outline) = font.outline_glyph(glyph) else {
                    continue;
                };
                let bb = outline.px_bounds();
                let ox = bb.min.x as i32;
                let oy = bb.min.y as i32;
                outline.draw(|dx, dy, cov| {
                    if cov <= 0.0 {
                        return;
                    }
                    let px = ox + dx as i32;
                    let py = oy + dy as i32;
                    if px < 0 || py < 0 || px >= buf_w as i32 || py >= buf_h as i32 {
                        return;
                    }
                    let idx = ((py as usize) * buf_w as usize + px as usize) * 4;
                    blend_premul_rgba(buf, idx, color, cov);
                });
            }
        }
    }
}

/// Source-over of an opaque `color` at coverage `cov` onto a premultiplied
/// RGBA8 pixel (byte order R, G, B, A).
fn blend_premul_rgba(buf: &mut [u8], idx: usize, color: [u8; 3], cov: f32) {
    let a = cov.clamp(0.0, 1.0);
    if a <= 0.0 || idx + 4 > buf.len() {
        return;
    }
    let inv = 1.0 - a;
    for k in 0..3 {
        let src = color[k] as f32 * a; // premultiplied source channel
        let dst = buf[idx + k] as f32 * inv;
        buf[idx + k] = (src + dst).round().clamp(0.0, 255.0) as u8;
    }
    let dst_a = buf[idx + 3] as f32;
    buf[idx + 3] = (a * 255.0 + dst_a * inv).round().clamp(0.0, 255.0) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_empty_is_zero() {
        let mut t = TextRaster::default();
        assert_eq!(t.measure("", 16.0), TextMetrics::default());
    }

    #[test]
    fn measure_latin_has_positive_width_when_font_present() {
        let mut t = TextRaster::default();
        let m = t.measure("Ab", 16.0);
        if m.width == 0.0 {
            eprintln!("no covering Latin font in this env — skipping");
            return;
        }
        assert!(m.width > 0.0 && m.height > 0.0 && m.baseline > 0.0);
    }

    #[test]
    fn cjk_candidate_char_detection_covers_common_scripts() {
        assert!(is_cjk_candidate_char('啊'));
        assert!(is_cjk_candidate_char('罐'));
        assert!(is_cjk_candidate_char('あ'));
        assert!(is_cjk_candidate_char('가'));
        assert!(!is_cjk_candidate_char('A'));
    }

    #[test]
    fn primary_family_scores_below_fallback() {
        let db = font_db();
        // Pick any two covering faces for 'A': tag one as primary, the other
        // not. The primary must produce a strictly lower score tuple.
        let covers: Vec<_> = db
            .faces()
            .filter(|f| face_covers(db, f.id, 'A'))
            .collect();
        if covers.len() < 2 {
            eprintln!("fewer than 2 Latin faces — skipping");
            return;
        }
        let a = &covers[0];
        let fam_a = a.families.first().map(|(n, _)| n.clone()).unwrap_or_default();
        let score_primary = face_score(a, 'A', Some(&fam_a)).0;
        let score_other = face_score(a, 'A', Some("This Family Does Not Exist")).0;
        assert_eq!(score_primary, 0, "primary family must be tier 0");
        assert!(score_other > 0, "non-primary must fall through to the lists");
    }

    #[test]
    fn set_preferred_family_flushes_cache() {
        let mut t = TextRaster::new(Some("Noto Sans".into()));
        t.entry_for_char('A');
        assert!(!t.cover.is_empty() && !t.entries.is_empty());
        t.set_preferred_family(Some("DejaVu Sans".into()));
        assert!(t.cover.is_empty(), "cover cache must flush on family change");
        assert!(t.entries.is_empty(), "loaded faces must flush on family change");
    }
}
