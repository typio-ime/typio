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
    → panel_scheduler (should_flush?) → present gate (frame callback + timer)
    → FluxPanel::draw_candidates
        → layout_candidates  (flux_text_measure, cached)
        → flux_text_draw     → glyph cache → atlas (FreeType raster)
        → offscreen flux image → readback → wl_shm buffer → Wayland compositor
```

Each stage has a distinct failure mode and a distinct probe:

| Stage | Failure mode | Grows over time? | Probe |
|------|---------------|------------------|-------|
| Glyph atlas | Atlas saturates → re-raster every frame | Yes (CJK working set) | `typio.panel.probe=debug` `atlas_clears` |
| Present gate | `wl_surface.frame` `done` never arrives → timer-paced fallback | Yes (after focus/occlusion) | `typio.panel.host` stall warning |
| Viewport fallback | No `wp_viewporter` → exact-size offscreen resize per page | Constant, not growing | startup `typio.wayland.viewporter` warning |
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

## Step 1 — Start a long diagnostic run

The lag is intermittent and may take hours to appear, so the run has to
stay quiet for a long time yet still capture the moment it happens. Three
rules make that work:

- **Use a release build.** A debug build inflates the timing numbers
  (`present_max_ms`, `total_max_ms`) and makes them meaningless. The
  *logical* signals (atlas clears, stall warnings, eviction deltas) are
  still valid in debug, but if you care about latency, build release.
- **Do *not* pass `--verbose`.** `-v` raises the floor to `debug` (per-tick
  panel-scheduler and per-key input lines); `-vv` to `trace` (per-frame
  timing). Over a day that is hundreds of MB to GB and buries the signal.
  Enable only the probe target with `RUST_LOG=typio.panel.probe=debug`; the
  `frame-callback stall` warning is `warn!` (>= the default `info` floor), so
  this captures the relevant long-run signals without turning on every debug
  event.
- **Split full vs. filtered output.** `tee` keeps a small `info`-floor full
  log as a fallback; `grep` writes a clean signal-only log you actually
  read. `--line-buffered` is required so the pipe flushes in real time.

```bash
cargo build --release -p typio-host --bin typio

RUST_LOG=typio.panel.probe=debug ./target/release/typio \
  --engine-dir ../typio-engines/typio-engine-compose \
  --engine-dir ../typio-engines/typio-engine-rime/build \
  --engine-dir ../typio-engines/typio-engine-sherpa/build \
  2>&1 \
  | tee typio-panel-full.log \
  | grep --line-buffered -E 'typio.panel.probe|frame-callback stall|wp_viewporter' \
  > typio-panel.log
```

Then just use the input method normally and let it run.

### Capture detail on demand when you feel the lag

Keep the run at the quiet `info` floor, and the moment you notice a
stutter, bump the level *temporarily* — no restart needed. The daemon
reloads its log floor live on `SIGUSR1` (raise one step) and `SIGUSR2`
(reset to the startup level):

```bash
kill -USR1 $(pidof typio)    # info → debug; reproduce the stutter now
# … page through candidates while it is laggy …
kill -USR2 $(pidof typio)    # back to quiet
```

Those few seconds of `debug`/`trace` land in `typio-panel-full.log` (not
the filtered log), so the main trail stays small while you still get
per-frame detail exactly when it matters. This is the recommended way to
chase an intermittent stutter over a long session.

> Running under the systemd user service instead? `journald` adds
> timestamps and rotation for free — drop the `tee`/`grep` plumbing and
> read with
> `journalctl --user -u typio | grep -E 'typio.panel.probe|stall'`.

Reproduce the lag (page through candidates for a while), then read the
two line types the probe emits.

**Per-window summary (every 120 presented frames):**

```text
DEBUG typio.panel.probe: panel probe window frames=1200 window=120 \
  candidate_count=9 present_max_ms=2.1 total_max_ms=4.8 slow_frames=0 \
  glyph_count=1873 glyph_cap=4096 atlas_clears=0 glyph_evictions_delta=0
```

**Immediate atlas-clear alert (fires the moment the atlas exhausts):**

```text
DEBUG typio.panel.probe: panel probe atlas clear atlas_clears=4 \
  glyph_count=8190 glyph_cap=16384
```

Read three numbers across successive windows:

- **`atlas_clears` climbing steadily** → the glyph atlas is thrashing
  (Dimension A). An occasional single bump is harmless; one every few
  windows during steady paging is the bug.
- **`glyph_evictions_delta` consistently `> 0`** → the glyph cache is over
  its working set; every evicted glyph re-rasterises via FreeType on its
  next appearance. Tolerable in bursts, suspicious when sustained.
- **`present_max_ms` climbing** → the cost is in GPU submit/readback or the
  SHM attach path (Dimension B), not glyphs.

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

**Deeper trace** (per-frame stage timing + glyph deltas). Once the probe
has pointed you here, get the per-frame breakdown without restarting the
long run — refine just these targets and bump the floor with `SIGUSR1`,
or start a fresh focused session with:

```bash
RUST_LOG="typio.panel.timing=info,typio.panel.text=debug" \
  ./target/release/typio --engine-dir … 2>&1 | tee typio-panel-deep.log
```

Watch `glyph_evictions_delta`, `atlas_clears_delta`, and `measure_ms` /
`draw_ms` per frame. Background:
[ADR-0019](../adr/0019-atlas-hash-compaction.md),
[ADR-0020](../adr/0020-atlas-reclamation-and-glyph-layer-modularization.md).
(Note: flux-text glyph-atlas tuning below applies to the historical GPU
render path; the panel now rasterises text on the CPU via `text_raster`.)

### Dimension B — Present gate / compositor back-pressure

**Signature:** `present_max_ms` climbing while `atlas_clears`/`evict` stay
flat; or the panel visibly freezes and then catches up in a burst.

**Why it happens:** after each SHM attach the daemon arms a
`wl_surface.frame` callback and uses it as a soft present gate. A healthy
callback wakes the panel at compositor refresh so the host does not spam
`wl_surface.attach`/`commit`. If the compositor stops delivering `done` —
popup occluded, on an unfocused output, or a buggy frame scheduler — the
panel waits only for the soft limit and then submits the latest coalesced
candidate state anyway. See
[ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md).

Current builds still report the condition: an uninterrupted
missing-callback episode older than the diagnostic threshold logs one
warning. Grep for it:

```bash
journalctl --user -u typio --since -1h | rg "frame-callback stall"
```

```text
panel: frame-callback stall — using timer-paced presents (compositor did not
  deliver wl_surface.frame done) stalled_ms=214.7 soft_limit_ms=20 \
  warn_after_ms=200 stall_count=37
```

A rising `stall_count` confirms the compositor is dropping frame
callbacks for the popup. The daemon recovers by timer-pacing candidates,
but a high count still points at the compositor's frame scheduling (file
upstream with the compositor name/version).

### Dimension B' — Missing `wp_viewporter`

**Signature:** constant per-page lag (not growing), present timing
correlates with `resized=true` in the timing log.

Without `wp_viewporter` the SHM buffer must equal the content exactly, so
every candidate-page width change resizes the offscreen image instead of just
updating a crop. Check at startup:

```bash
wayland-info | grep -i viewport
journalctl --user -u typio | rg "wp_viewporter"
```

If the daemon logged *"compositor lacks wp_viewporter"*, this is your cause.
There is no host-side equivalent to the grow-only crop without the protocol.
Use a compositor that advertises `wp_viewporter`.

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
  else "lacks wp_viewporter"? → Dimension B'                → switch compositor
  else present_max_ms rising? → Dimension B (GPU/readback/SHM path)
  else (all flat, lag real)   → Dimension C (engine)        → watchdog stage attribution
```

## What to include in a bug report

- `typio --version` and confirmation you rebuilt against local optics
- A `typio.panel.probe=debug` log window spanning the lag (several
  `panel probe window` lines so the trend is visible)
- Any `frame-callback stall` lines and the final `stall_count`
- `wayland-info | grep -i viewport` output
- Compositor name and version
- For Dimension C: a `RuntimeState` snapshot taken during the lag (see
  [troubleshooting](troubleshooting.md#wayland-runtime-diagnostics))

## Related

- [Candidate panel behavior](../explanation/candidate-panel-behavior.md)
- [Performance strategy](../explanation/performance-strategy.md)
- [ADR-0015 — Candidate popup lag final fixes](../adr/0015-candidate-popup-lag-final-fixes.md)
