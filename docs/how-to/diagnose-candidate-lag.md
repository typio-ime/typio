# How to Diagnose Candidate-Switching Lag

**Symptom this guide addresses:** typing stays responsive, but *switching
or paging candidates* (arrow keys, page keys, space) becomes progressively
laggy the longer the daemon runs — frames feel delayed, skipped, or the
panel updates in bursts.

"Lag that grows over time" almost always means a resource that accumulates
or a back-pressure loop that wedges — not a constant per-frame cost. This
guide walks the candidate-render pipeline end to end, names the three
places the lag can originate, and gives you a probe for each so you can
tell them apart instead of guessing.

## The pipeline at a glance

```text
key → engine (libtypio/Rime) → CompositionState.candidates
    → panel_scheduler (should_flush?) → present throttle (frame callback)
    → FluxPanel::draw_candidates
        → layout_candidates  (flux_text_measure, cached)
        → flux_text_draw     → glyph cache → atlas (FreeType raster)
        → flux_frame_present → Vulkan swapchain → Wayland compositor
```

Each stage has a distinct failure mode and a distinct probe:

| Stage | Failure mode | Grows over time? | Probe |
|------|---------------|------------------|-------|
| Glyph atlas | Atlas saturates → re-raster every frame | Yes (CJK working set) | `TYPIO_PANEL_PROBE` `atlas_clears` |
| Present throttle | `wl_surface.frame` `done` never arrives → panel wedged | Yes (after focus/occlusion) | `typio.panel.host` stall warning |
| Swapchain | No `wp_viewporter` → rebuild per page | Constant, not growing | startup `typio.wayland.viewporter` warning |
| Engine | Rime userdb / state grows | Yes | watchdog stage attribution |

## Step 0 — Confirm you are on a fixed build

Two of the classic causes are already fixed in current source, so the
*first* thing to rule out is that you are running a stale binary:

- The **glyph-atlas thrash** (O(N) re-rasterise of every cached glyph on
  every atlas exhaustion) was replaced by an O(1) atlas clear — see
  `optics/libs/flux/text/src/atlas.c` and
  [ADR-0020](../adr/0020-atlas-reclamation-and-glyph-layer-modularization.md).
- A **file-descriptor leak** on keymap events that silently dropped panel
  frames once the fd table filled — see the
  [troubleshooting note](troubleshooting.md#candidate-navigation-becomes-sluggish-after-extended-runtime).

Rebuild against the local optics monorepo and reinstall before
investigating further:

```bash
cargo build --release -p typio-host --bin typio
typio --version
```

## Step 1 — Turn on the panel probe

The panel probe is a single-env-var, always-on stderr summary built for
exactly this investigation. It needs no `RUST_LOG` knowledge:

```bash
TYPIO_PANEL_PROBE=1 typio --verbose 2>&1 | tee typio-panel.log
```

Reproduce the lag (page through candidates for a while), then read the
two line types it emits.

**Per-window summary (every 120 presented frames):**

```text
panel-probe: frames=1200 window=120 cands=9 present_max_ms=2.1 total_max_ms=4.8 \
  slow_frames=0/120 glyph_count=1873 glyph_cap=4096 atlas_clears=0 evict/win=0
```

**Immediate atlas-clear alert (fires the moment the atlas exhausts):**

```text
panel-probe: ATLAS CLEAR #4 (glyph atlas exhausted — next frames re-rasterise \
  visible glyphs; sustained clears = thrash) glyph_count=8190 glyph_cap=16384
```

Read three numbers across successive windows:

- **`atlas_clears` climbing steadily** → the glyph atlas is thrashing
  (Dimension A). An occasional single bump is harmless; one every few
  windows during steady paging is the bug.
- **`evict/win` consistently `> 0`** → the glyph cache is over its working
  set; every evicted glyph re-rasterises via FreeType on its next
  appearance. Tolerable in bursts, suspicious when sustained.
- **`present_max_ms` climbing** → the cost is in `flux_frame_present`,
  i.e. compositor / swapchain back-pressure (Dimension B), not glyphs.

If all three stay flat while the lag is real, the cost is upstream of the
panel — suspect the engine (Dimension C).

## Step 2 — Match the signal to a dimension

### Dimension A — Glyph atlas / GPU text cache

**Signature:** `atlas_clears` rising; `total_max_ms` spikes coincide with
the clears.

**Why it happens:** a long CJK session touches thousands of distinct Han
glyphs. The atlas is 4096×4096 and the glyph hash table holds ~8192 live
entries (`optics/libs/flux/text/src/text_internal.h`). When the working
set exceeds what fits, the atlas clears and the next frames re-rasterise
every visible glyph. The current O(1) clear keeps a single clear cheap;
*sustained* clears mean the working set genuinely exceeds capacity.

**Deeper trace** (per-frame stage timing + glyph deltas):

```bash
RUST_LOG="typio.panel.timing=info,typio.panel.text=debug" typio --verbose
```

Watch `glyph_evictions_delta`, `atlas_clears_delta`, and `measure_ms` /
`draw_ms` per frame. Background:
[ADR-0019](../adr/0019-atlas-hash-compaction.md),
[ADR-0020](../adr/0020-atlas-reclamation-and-glyph-layer-modularization.md),
and [Vulkan/flux rendering](../explanation/vulkan-flux-rendering.md).

### Dimension B — Present throttle / compositor back-pressure

**Signature:** `present_max_ms` climbing while `atlas_clears`/`evict` stay
flat; or the panel visibly freezes and then catches up in a burst.

**Why it happens:** after each present the daemon arms a
`wl_surface.frame` callback and skips presenting until the compositor
sends `done` (this caps the present rate at the refresh rate so the
synchronous `vkQueuePresentKHR` never blocks the loop — see
[ADR-0010](../adr/0010-non-blocking-candidate-popup-present.md)). If the
compositor stops delivering `done` — popup occluded, on an unfocused
output, or a buggy frame scheduler — the throttle would wedge and every
later candidate update is dropped.

Current builds defend against this: an outstanding callback older than
`PANEL_FRAME_CALLBACK_TIMEOUT` (200 ms) force-clears the throttle and
logs a one-shot warning. Grep for it:

```bash
journalctl --user -u typio --since -1h | rg "frame-callback stall"
```

```text
panel: frame-callback stall — forcing present (compositor did not deliver
  wl_surface.frame done) stalled_ms=214.7 timeout_ms=200 stall_count=37
```

A rising `stall_count` confirms the compositor is dropping frame
callbacks for the popup. The daemon now recovers automatically, but a
high count points at the compositor's frame scheduling (file upstream
with the compositor name/version).

### Dimension B′ — Missing `wp_viewporter`

**Signature:** constant per-page lag (not growing), present timing
correlates with `resized=true` in the timing log.

Without `wp_viewporter` the swapchain buffer must equal the content
exactly, so every candidate-page width change rebuilds the swapchain
(`vkDeviceWaitIdle` + WSI roundtrips). Check at startup:

```bash
wayland-info | grep -i viewport
journalctl --user -u typio | rg "wp_viewporter"
```

If the daemon logged *"compositor lacks wp_viewporter"*, this is your
cause. There is no host-side fix — the grow-only swapchain
([ADR-0013](../adr/0013-grow-only-popup-swapchain.md)) requires the
protocol. Use a compositor that advertises `wp_viewporter`.

### Dimension C — Engine (libtypio / Rime)

**Signature:** probe + timing all flat, but the lag is real; the watchdog
attributes stalls to a key-dispatch stage rather than `Present`.

Candidate paging round-trips through the engine. If Rime's user
dictionary or per-session state grows, selection/paging slows
independent of rendering. Confirm by watching which `LoopStage` the
watchdog reports during the lag (see
[Event Loop Scheduling](../explanation/event-loop-scheduling.md) and
[Watchdog](../explanation/watchdog.md)). A stall in the key-dispatch /
FFI path, not `Present`, points at the engine. Try
`typio rime deploy` and, as a test, a fresh Rime user directory.

## Step 3 — Decision tree

```text
atlas_clears rising?          → Dimension A (glyph atlas)   → ADR-0019/0020
  else stall warning present? → Dimension B (frame callback) → compositor frame sched
  else "lacks wp_viewporter"? → Dimension B′                → switch compositor
  else present_max_ms rising? → Dimension B (swapchain back-pressure)
  else (all flat, lag real)   → Dimension C (engine)        → watchdog stage attribution
```

## What to include in a bug report

- `typio --version` and confirmation you rebuilt against local optics
- A `TYPIO_PANEL_PROBE=1` log window spanning the lag (several
  `panel-probe` lines so the trend is visible)
- Any `frame-callback stall` lines and the final `stall_count`
- `wayland-info | grep -i viewport` output
- Compositor name and version
- For Dimension C: a `RuntimeState` snapshot taken during the lag (see
  [troubleshooting](troubleshooting.md#wayland-runtime-diagnostics))

## Related

- [Candidate panel behavior](../explanation/candidate-panel-behavior.md)
- [Performance strategy](../explanation/performance-strategy.md)
- [Vulkan / flux rendering](../explanation/vulkan-flux-rendering.md)
- [ADR-0015 — Candidate popup lag final fixes](../adr/0015-candidate-popup-lag-final-fixes.md)
