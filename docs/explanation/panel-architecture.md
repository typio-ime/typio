# Panel Architecture

Typio has one floating IME UI: the **Panel**. It may look like several
different things during use — candidate list, engine/profile indicator, voice
recording status — but those are different **owners** of the same Panel, not
different windows.

This document explains how the Panel system is structured: layer boundaries,
ownership arbitration, and position-anchor mechanics. Term definitions live in
the [Glossary](../reference/glossary.md); naming decisions are recorded in
[ADR-0014](../adr/0014-canonical-panel-vocabulary.md) and [ADR-0017](../adr/0017-positioned-ui-arbitration.md).

## Layer boundaries

The Panel system is split into two large areas.

### Frontend policy

The frontend knows about Wayland focus, input-method commits, voice state,
engine mode changes, and browser anchor quirks. That policy belongs to the
daemon's frontend.

The main frontend policy object is the **Panel Coordinator**. It lives in the
`typio-host-types` crate rather than in the daemon crate, together with the
panel scheduler, so the frontend and the presentation layer can both reach it
without pulling in a Wayland dependency. It answers:

- who currently owns the Panel;
- whether a new producer may replace the current owner;
- whether a hide event is stale;
- whether positioned UI must wait for an anchor;
- whether to send an anchor probe.

### Panel rendering

Rendering belongs to the platform layer. It answers:

- how candidates and status banners become geometry;
- how geometry becomes canvas draw commands;
- how glyphs are shaped and cached;
- how the CPU canvas framebuffer is attached through shared-memory buffers.

Rendering does not know whether content came from voice, indicator, or candidate
composition. Ownership policy runs before the renderer is called.

The GPU era is over: the Panel paints on a CPU canvas and presents only through
shared-memory buffers. There is no GPU glyph atlas and no Vulkan or dma-buf
present path, and neither may return. The retired mechanisms and the reasons
they must stay retired are tabulated in the
[Panel Rendering Blueprint](../architecture/panel-rendering.md).

Present scheduling is a two-state idle/dirty value with no retry state and no
durable retry flag: if the compositor has not released a buffer, the frame stays
dirty and is retried from the update path on a later step.

## Owners and mutual exclusion

The Panel has exactly one visible owner at a time:

| Owner | Producer | Typical content |
|---|---|---|
| Candidate | keyboard composition | candidate list and selection highlight |
| Indicator | engine/profile changes | active engine and profile label |
| Voice | voice session | loading, recording, processing, unavailable, error |

The current policy is temporal: a later owner replaces the current owner. A
hide request only hides the owner that issued it. This prevents stale events,
such as an old indicator timer, from hiding a newer candidate panel.

Candidate UI has one extra rule: when candidates arrive, pending positioned
status UI is cancelled. Typing is the highest-signal evidence that the user is
actively composing, so candidate UI should not be delayed behind an old status.

## Position anchors

The compositor positions the input-popup surface near the text input area, but
the input method does not own global caret coordinates. The Panel therefore
tracks whether the current activation has a trustworthy **position anchor**.

Each activation receives an **anchor generation**. That generation becomes ready
when either:

- the compositor sends a caret rectangle for the current activation;
- candidates successfully present for the current activation.

Candidates can establish the anchor because they are input-driven. Browsers
often update caret rectangles only after real input-method traffic, so candidate
placement is usually reliable even when out-of-band status UI is not.

## Anchor probe

Indicator and voice status are out-of-band UI. They may need a cursor position
even though the application has not recently updated text-input state.

When such positioned status UI is requested without a ready anchor, the Panel
Coordinator can send one **anchor probe** for the current generation: an empty
preedit update followed by a no-op commit that carries the current protocol
serial.

The probe is intentionally no-op from the user's point of view. Its purpose is
to make clients that refresh caret rectangles on input-method commits, notably
Chrome and Firefox, send a fresh caret rectangle.

The probe is enabled by default and waits a bounded time for the anchor to
become ready; both the switch and the timeout are display settings of the
configuration, documented with their types and defaults in the
[Configuration Reference](../reference/configuration.md).

If the anchor still does not become ready before the timeout, the pending status
UI is dropped rather than shown at a stale location.

## The data flow

| Stage | What happens |
|---|---|
| Producers → Panel Coordinator | candidate composition, the indicator, and voice status each request the Panel |
| Panel Coordinator → Panel | owner arbitration approves or defers the request |
| Panel Coordinator → Wayland input-method connection | anchor readiness checks and the anchor probe |
| Panel → Panel Content | approved content becomes a content model |
| Panel Content → Panel Geometry | content becomes measured geometry |
| Panel Geometry → Paint | geometry becomes canvas draw commands |
| Paint → Panel Surface | the painted canvas becomes a shared-memory buffer |
| Panel Surface → input-popup surface | the buffer is attached to the popup surface |

The important split is that producer arbitration happens before rendering.
Rendering is downstream and should not contain ownership policy.

## Design rules

1. Do not let producers call the renderer directly. Route through the Panel
   Coordinator.
2. Do not put owner or anchor policy in the rendering and presentation modules.
   Those modules are rendering and presentation.
3. Do not let stale owner events hide the current owner.
4. Do not show positioned status UI at an untrusted anchor.
5. Treat candidates as both UI content and anchor-producing evidence.
6. Keep *popup* reserved for protocol-level naming.

## See also

- [Candidate Panel Behavior](candidate-panel-behavior.md) — UI-level lifecycle of the candidate box (show/hide, anchor, input → visible effect)
- [Glossary](../reference/glossary.md) — term definitions
- [Frontend Graphics](frontend-graphics.md) — render pipeline
- [Wayland Input Method Protocol](wayland-input-method.md)
- [ADR-0014: Canonical panel vocabulary and module ontology](../adr/0014-canonical-panel-vocabulary.md)
- [ADR-0017: Positioned UI arbitration for panel owners](../adr/0017-positioned-ui-arbitration.md)
