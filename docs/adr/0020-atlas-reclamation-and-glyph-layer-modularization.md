---
id: ADR-0020
title: "Atlas texture reclamation and glyph-layer modularization"
status: accepted
date: 2026-06-02
scope: panel
---

# ADR-0020: Atlas texture reclamation and glyph-layer modularization

- **Status**: Accepted
- **Date**: 2026-06-02
- **Deciders**: Project maintainers
- **Supersedes**: [ADR-0019](0019-atlas-hash-compaction.md)
- **Relates to**: [ADR-0012](0012-glyph-atlas-shared-texture.md), [ADR-0016](0016-per-glyph-font-fallback.md), [ADR-0009](0009-long-term-performance-optimizations.md)

## Context

The "candidate list lags / goes stale after the IME has been used for a while"
symptom recurred after ADR-0019. Re-investigation found ADR-0019's root-cause
attribution **inverted**.

ADR-0019 assumed the dominant time-dependent cost was hash-table probe-chain
degradation, and stated as an accepted negative that *"dead entries' atlas
texture space is never reclaimed … the hash table compacts long before texture
exhaustion would occur."* That ordering is backwards:

- The hash table holds `GLYPH_SLOT_CAP = 131072` slots; its 75 % reclaim
  threshold is ~98 K **distinct glyphs**.
- The atlas **texture** is a single 2048² R8 image. At a 16 px default (CJK
  bitmaps ~18 px, +1 px gutter) it holds ~9–10 K glyphs; on a HiDPI output
  (scale 2, ~36 px) only ~3–4 K.
- A daily-driver CJK session types a few thousand distinct Han characters
  *routinely* — so the **texture saturates first**, long before `live_count`
  approaches the hash threshold.

Because the shelf packer (`glyph_pack.c`) only ever advances and the old
compaction rebuilt **only the hash table** — explicitly abandoning texture
space and never resetting the packer — once the image filled it could never
recover. Every subsequent new glyph failed to pack and was recorded
non-drawable, so new/rare candidates rendered **blank permanently** until a
restart or font reload. And since `live_count` rarely reached 75 %, the
compaction path itself almost never ran.

A second, independent cost compounded it: per-codepoint fallback resolution
(`FcFontSort` over every installed font) ran **uncached** on every layout
re-creation under LRU churn. The coverage-keyed `fallback_cache.{c,h}` built to
prevent exactly this was never wired into the hot path — dead code.

All of this lived in a single 1600-line `text_shaper.c` owning six unrelated
concerns and four independently-evolving caches. A file that large is precisely
where "packer never reset" and "cache written but never wired" slip through
review.

## Decision

### 1. Atlas reclamation = full rebuild (reverses ADR-0019's accepted negative)

`glyph_atlas_reclaim()` rebuilds the atlas wholesale — image, slot table, packer
cursor, and `live_count` are all reset — and the next draw re-rasterises the
visible page's glyphs lazily into the fresh, zeroed image. The work is bounded
by the **page size** (tens of glyphs), not session history.

Trigger: 75 % hash load **OR** a new `packer_exhausted` flag. The flag is set
only when a glyph that *would* fit an empty atlas fails to pack the current one
(the never-fits case — a glyph larger than the atlas — is excluded so it cannot
thrash the rebuild), which rate-limits the rebuild to once per fill cycle.

It runs at the top of `panel_render()` behind a `flux_device_wait_idle()`: no
frame command buffer references the image yet and the previous frame's GPU work
has drained, so tearing the in-use image down is safe.

### 2. Per-codepoint fallback memo (delete the dead coverage cache)

Fallback resolution is memoised by `(codepoint, weight)` in `font_resolve.c`,
collapsing `FcFontSort` to once per script while preserving the existing
per-codepoint, FT-verified, multi-candidate behaviour. The superseded,
never-wired coverage-keyed `fallback_cache.{c,h}` (and its test) are deleted.

### 3. Glyph-layer modularization

`text_shaper.c` (1609 → ~380 lines) is split into focused modules, each owning
one resource and documenting its lifecycle contract in its header:

| Module | Owns |
|---|---|
| `glyph_upload.*` | Persistent Vulkan staging context (pool/buffer/fence). |
| `glyph_atlas.*` | Coverage texture + hash table + reclamation. |
| `font_cache.*` | `FT_Face` + `hb_font_t` object cache (monotonic `font_id`). |
| `font_resolve.*` | Fontconfig: descriptor parse, family→file, per-codepoint fallback. |
| `text_shaper.c` | Shaping + draw + the panel-facing `typio_text_atlas_*` facade. |

The ADR-0019 hash-compaction LRU-iteration API
(`panel_render_ctx_entry_count` / `panel_render_ctx_entry_shapes`) is removed —
a full rebuild needs no live-set walk.

### 4. Cache discipline + diagnostics

Every cache header answers four questions: **Bound** (capacity), **Evict**
(who goes when full), **Reclaim** (how the underlying resource is returned), and
**Observe** (counters). The original bug was a missing Reclaim plus a missing
Observe. Cumulative counters (atlas rebuilds, glyphs rasterised, fallback memo
hit/miss) are exposed via `typio_text_shaper_get_diag()` and logged by
`typio_text_shaper_log_diag()`, wired into the panel slow-render path so a stall
prints atlas saturation and fallback-resolution cost inline.

## Alternatives considered

- **Keep hash-only compaction (ADR-0019).** Rejected — it never addressed the
  binding constraint (texture space) and seldom ran.
- **Repack live glyphs eagerly on reclaim.** Rejected — re-rasterises the whole
  LRU working set up front; lazy re-raster pays only for glyphs actually drawn.
- **Grow the atlas / add atlas pages.** Rejected — unbounded VRAM and more
  bookkeeping for a working set that is tiny versus capacity, so a periodic
  rebuild is rare and cheaper.
- **Reset the packer in place without rebuilding the image.** Rejected — stale
  gutter pixels could bleed under bilinear sampling; a fresh zeroed image is
  cleaner and the rebuild is infrequent.

## Consequences

- Positive: the atlas now recovers on its own. New glyphs no longer render blank
  after extended use; no restart or font reload required.
- Positive: fallback `FcFontSort` is paid ~once per script, not per layout
  re-creation; Fontconfig churn under LRU pressure drops.
- Positive: the glyph/font layer is testable and reviewable per module, and the
  long-term-performance failure mode is observable via the diag counters.
- Trade-off: a rebuild forces a one-off re-rasterisation of the visible page
  (a few ms), but only once per fill cycle (thousands of distinct glyphs apart).
- Trade-off: a brief `flux_device_wait_idle()` on rebuild — bounded and rare,
  same envelope ADR-0019 already accepted for compaction.
- Removed: ADR-0019's 4× hash capacity bump stays (cheap), but its compaction
  machinery and the LRU-iteration API are gone.
