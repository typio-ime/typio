---
id: ADR-0032
title: "Tray icon composition — language base + modality overlays"
status: accepted
date: 2026-06-13
scope: control
---

# ADR-0032: Tray icon composition — language base + modality overlays

- **Status**: Accepted
- **Date**: 2026-06-13
- **Deciders**: Project maintainers

## Context

ADR-0031 made **language** the user-facing switch unit and brought the tray
menu, tooltip, and on-screen indicator in line with it. The tray *icon* itself
was left engine-centric, which surfaced two problems:

1. **Two simultaneously-active slots, one icon.** ADR-0026 establishes that the
   keyboard and voice slots are orthogonal and active at the same time, not
   alternatives. A single `IconName` cannot represent both, so whichever slot
   "owns" the icon hides the other.
2. **Layout-only languages have no engine to source an icon.** A language like
   Moroccan Darija (`ary`) resolves to an empty keyboard slot (ADR-0031). With
   icon resolution sourced purely from the engine, the icon fell through to the
   *off* glyph — making an active language look disabled.

The underlying issue: the icon was asked to encode both **state** (what is
processing input right now — high-frequency, glanceable) and **identity** (which
engine backend is selected — low-frequency, set-and-forget). Forcing identity
into the state slot is what produced the "show keyboard *or* voice" dead-end.

SNI (StatusNotifierItem) already exposes more than one icon channel
(`IconName`/`IconPixmap`, `OverlayIconName`/`OverlayIconPixmap`,
`AttentionIconName`); the host was sending all but the base channel empty.

## Decision

Compose the tray icon from layered channels with distinct jobs, anchored on the
language as the stable identity. Detail (which engines, which schema) stays in
the tooltip and menu, which are already language-first.

### Base icon — the keyboard dimension, language as the floor

The base icon (`IconName` or `IconPixmap`) answers "what am I typing with". It
is resolved by a single precedence chain (owned by the state controller),
most-specific first:

1. dynamic mode/schema icon (same engine) — most specific runtime state
2. engine manifest `icon`
3. `[languages.<tag>].icon` config override
4. **language text badge** — the floor; always available
5. generic `typio-keyboard-symbolic` (active, no icon anywhere)
6. `typio-keyboard-off-symbolic` — **only** when nothing is active (no language
   *and* no engine)

The change from ADR-0031's chain is layer 4: the floor is no longer the off
glyph but a **language badge** — a 1–3 glyph label in the language's own script
(`中` / `あ` / `الد` / `EN`), rendered by the host. This makes the icon *be* the
language: it works for layout-only languages with zero engines, scales to any
BCP 47 tag automatically, and is stable across engine/mode churn (it only
changes when the language changes). The controller exposes the resolution as
either a freedesktop icon name or a badge-text flag; the tray renders
accordingly, and the generic name from layer 5 is retained as a fallback when
badge rasterisation is unavailable.

### Badge rendering — CPU rasterisation to `IconPixmap`

Badges are drawn by a standalone unit (`src/tray/icon_badge.c`) using
FreeType + HarfBuzz + Fontconfig to rasterise the glyphs into an ARGB32
`IconPixmap` (SNI `a(iiay)`, **network/big-endian** byte order, with 22px and
44px entries for HiDPI). HarfBuzz shaping is mandatory, not cosmetic: Arabic
badges such as الد require contextual joining that isolated-glyph lookup would
get wrong. Fontconfig matches a covering face by the badge's first codepoint, so
CJK, Arabic, and Latin badges each resolve to a usable font. Glyphs are white
with a one-pixel dark halo (the dilation of the coverage) so the badge reads on
both light and dark panels.

The unit is **independent of the flux/Vulkan/Wayland panel stack** (ADR-0012):
the GPU atlas is an R8 coverage texture for the Vulkan composer and cannot feed
D-Bus, so the badge does not drag the GPU panel into the tray — it shares only
the underlying font libraries, which the meson build now resolves whenever
either the panel or the tray is enabled. (The project's standard build couples
Wayland and systray; a fully Wayland-less build is out of scope, as the test
suite assumes the Wayland frontend.) When FreeType/Fontconfig cannot produce a
bitmap, the base icon falls back to the layer-5 generic name — the badge is an
enhancement, never a hard dependency.

### Overlay icon — the voice dimension

The voice slot is surfaced on `OverlayIconName`, composited by the host desktop
onto the corner of the base icon. Its job is *presence*, not identity: it
indicates a voice engine is configured/available, using the stock
`audio-input-microphone-symbolic`. It never competes for the base icon, which
keeps the keyboard dimension legible.

**Brand identity for voice (Whisper vs Sherpa) is not encoded in the icon.**
Two brand logos composited at 22px are illegible and semantically muddled.
Backend identity is low-frequency state and lives in the menu's voice-engine
list (ADR-0026 symmetry with the keyboard list) and the tooltip.

### Active-state semantics

"A language is active" counts as active for `Status` (Active vs Passive) and for
the on/off icon decision, so a layout-only language never shows the passive/off
presentation.

### Deferred: recording / attention state

A "voice is recording now" state (which would drive `Status=NeedsAttention` +
`AttentionIcon`, or swap the base to the voice engine's icon while recording) is
**deferred because there is no data source for it on the host**. Voice engines
are out-of-process workers (ADR-0028); push-to-talk and capture are handled
inside libtypio/the engine, and the host observer callbacks expose only
`voice_engine_changed` and the engine-lifecycle `availability` enum
(READY/STARTING/ERROR/…), not a recording transient. Encoding a recording
indicator today would mean inventing state the host does not have. If a future
libtypio adds a voice-activity callback, it composes cleanly into the existing
overlay/attention channels.

## Alternatives considered

- **Keep one icon, pick keyboard or voice.** Rejected: it structurally hides one
  of two simultaneously-active slots (ADR-0026).
- **Two separate tray items (keyboard + voice).** Rejected: consumes two slots,
  and they are one input method, not two apps.
- **Composite both engine logos into one pixmap.** Rejected: illegible at tray
  sizes; forces always-on rasterisation; degrades poorly when a slot is empty.
- **Ship a themed icon file per language.** Rejected: does not scale to the BCP
  47 space, and "the English icon" invites the flag trap (language ≠ country;
  Darija ≠ the Morocco flag). The generated badge is per-language for free;
  `[languages.<tag>].icon` remains for deliberate per-language customisation.
- **Render badges via the flux GPU atlas.** Rejected: the atlas is an R8
  coverage texture owned by the Vulkan composer and tied to `enable_wayland`;
  the tray is a separate sd-bus surface that needs CPU ARGB32.

## Consequences

- Positive: the icon *is* the language; layout-only languages (Darija) get a
  meaningful, stable icon with zero engines.
- Positive: keyboard and voice are both visible (base + overlay) without either
  hiding the other.
- Positive: badge generation scales to any tag; no per-language asset pipeline.
- Trade-off: the badge unit makes FreeType/HarfBuzz/Fontconfig dependencies of
  the tray, not just the Wayland panel; the meson build resolves them for either
  consumer. They are ubiquitous and lightweight, and the badge degrades to a
  generic icon name when absent.
- Trade-off: `IconPixmap` must be emitted big-endian and at multiple sizes — a
  known SNI footgun, contained in one unit and covered by a data-contract test.
- Negative (accepted): no recording/attention indicator until libtypio exposes
  voice activity; the channels are reserved for it.

## Related

- [ADR-0026](0026-modality-explicit-engine-control-surface.md) — keyboard/voice
  are orthogonal, simultaneously-active slots; menu symmetry.
- [ADR-0031](0031-language-first-switching-surface.md) — language as the switch
  unit; the icon precedence chain this ADR extends.
- [ADR-0012](0012-glyph-atlas-shared-texture.md) — the GPU atlas the badge unit
  deliberately does *not* reuse.
- [ADR-0028](0028-direct-ipc-engine-workers.md) — out-of-process voice workers;
  why recording state is not on the host.
- [Configuration Reference](../reference/configuration.md) — `[languages.<tag>].icon`.
