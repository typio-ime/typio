# Candidate Panel Behavior

This document describes the **UI-level lifecycle of the candidate
box**: what the user sees, when the box appears and disappears, how
input maps to visible state, and how the box behaves when the
compositor or engine stalls. It is the user-behavior counterpart to
[Panel Architecture](panel-architecture.md), which covers multi-owner
arbitration, and [Frontend Graphics](frontend-graphics.md), which
covers the render pipeline.

The vocabulary follows [ADR-0014](../adr/0014-canonical-panel-vocabulary.md):
**Panel** is the floating IME surface as a whole; **Candidate Zone**
is the region that lists the engine's candidates. "Candidate window"
is a user-facing prose synonym, not a code identifier.

## What the User Sees

When the Panel is owned by composition, it shows the Candidate Zone:

| Region | Contents | Driven by |
|---|---|---|
| **Candidate Zone** | A vertical list of candidates with a muted index label (`1`…`9`, `0`) before each entry. The selected candidate is highlighted. | The engine's published candidate list and the index of its selected entry |

Inline preedit is not rasterised into the Panel. The daemon sends it to the
compositor as an input-method preedit update, and the focused application
renders it at the insertion point. Preedit and candidates therefore update
through independent presentation paths: a pinyin engine after one keystroke
can show inline preedit with no Panel, while a completion engine can show
candidates with empty preedit.

## Lifecycle States

The Candidate Zone moves through four logical states: hidden, waiting for an
anchor, visible, and hidden again. Waiting for an anchor is a phase inside the
visible path rather than a separate surface — while the host waits for a
trustworthy placement rectangle, the engine already owns the composition, and
the Panel is only withheld from the compositor.

| State | Trigger | Next state |
|---|---|---|
| Hidden | the engine publishes a non-empty composition | Waiting for anchor |
| Waiting for anchor | the compositor delivers a placement rectangle for the current activation | Visible |
| Waiting for anchor | the bounded probe wait expires and a placement rectangle was seen earlier in this activation | Visible (caret fallback) |
| Waiting for anchor | the bounded probe wait expires with no placement rectangle to fall back on | Hidden |
| Visible | a composition update or a candidate page change | Visible |
| Visible | commit, focus loss, or a runtime reset | Hidden |
| Visible | reactivation refreshes the anchor | Waiting for anchor |

There is no separate *retry* state. A frame that the host declines to draw
because the compositor has not released a buffer is retried from the dirty
schedule on a later reactor step; the schedule itself records only whether
newer candidate state still needs to reach the compositor.

### Hidden

No Panel surface is mapped. The compositor is not asked to position
anything. The host keeps draining keyboard events normally; if the
engine emits a non-empty composition, the lifecycle restarts at
Waiting for anchor.

### Waiting for Anchor

The engine has produced candidates, but the compositor has not yet
provided a trustworthy placement rectangle for the current activation.
The host sends an **anchor probe** — an empty preedit followed by a
no-op commit that carries the current protocol serial — to make browsers and
other clients that refresh caret rectangles on input-method traffic send a
fresh placement rectangle. The probe is bounded by a configurable probe
timeout with a floor and a ceiling. Candidate popups use immediate fallback
placement so a slow caret rectangle does not delay the first frame; the probe
wait mainly applies to status overlays.

If the anchor becomes ready within the timeout, the Panel maps and the
state becomes Visible. If the timeout expires, the host applies the
**caret fallback**: if the compositor has *ever* sent a caret rectangle
for this activation, that cached rectangle is trusted; otherwise the
update is discarded and the state returns to Hidden.

### Visible

The Panel surface is mapped near the caret. Each new composition output
repaints the Candidate Zone without re-arming the anchor
probe when candidate content or selection changes. Preedit-only edits use the
separate Wayland text path and do not require a Panel repaint. The host marks
the Panel dirty and the event loop flushes candidate redraws in the current or
next reactor step.

### Hidden Again

The Candidate Zone is torn down when any of these happens:

- The engine emits a commit output record.
- The input context loses focus (a focus-out transition is applied).
- The runtime resets the context — for example on a soft
  pause or resume-from-suspend hard boundary.
- A composition output arrives with no candidates. Inline preedit may remain
  visible in the focused application.

Tearing the Panel down detaches the shared-memory buffer so no stale popup
shadow remains beside the caret.

## How Input Maps to Visible State

The host and the engine share responsibility for navigating the
Candidate Zone. The engine declares, in its composition output, which key
groups the host may manage on its behalf. Four capabilities exist:

- **Navigation** — the arrow keys move the highlighted candidate by one,
  clamped at the list edges.
- **Commit** — Space commits the currently highlighted candidate.
- **Index pick** — the digit keys commit the candidate at that index directly.
- **Raw commit** — Enter commits the preedit as-is, ignoring candidate
  selection.

When the engine declares no capabilities at all, the host still intercepts
the arrow keys as long as candidates exist — the historical default
that keeps arrow navigation working in engines that pre-date the capability
contract. Number keys, Space, and Enter then fall through to
the engine's own key processing.

Index-pick keys are filtered against the actual candidate count: the key for
the sixth index with only four candidates is forwarded to the engine rather
than swallowed, so the engine can interpret it (for example as a literal digit
in the preedit).

## Position and Anchor

The Panel is placed by the compositor near the text-input rectangle.
The host does not own global caret coordinates; it depends on the
compositor sending a caret rectangle for the active input-popup role.

Each focus activation receives a fresh **anchor generation**. The
generation becomes *ready* when either:

1. The compositor sends a placement rectangle for the current
   activation, **or**
2. The Candidate Zone successfully presents for the current
   activation.

The second path exists because candidates are input-driven: browsers
such as Firefox and Chrome frequently update caret rectangles only
after real input-method traffic, so the candidate Panel usually
appears in the right place even when out-of-band indicator UI does
not. A successful first present retroactively marks the anchor
trustworthy for any subsequent positioned UI in the same activation.

Reactivation (clicking between two text fields in the same window)
refreshes the anchor generation and invalidates the last-presented candidate
snapshot. The host re-sends the probe and re-presents non-empty candidates
even when their contents and sequence number did not change. This is required
because the compositor may unmap the popup during the focus handoff without
changing any host-side presentation state. The Candidate Zone therefore uses
fresh placement instead of silently retaining the previous field's cached
position or remaining absent until another composition update.

## What the User Sees During Stalls

A frozen or slow compositor does not corrupt committed text. Input
events continue to queue on the Wayland connection while a frame is skipped,
so navigation and selection stay correct even when the visible
highlight briefly lags behind. This is the central design property
established in [ADR-0006](../adr/0006-resilient-candidate-popup-present.md)
and revisited in [Frontend Graphics](frontend-graphics.md#input-correctness).

The current panel path renders entirely on the CPU into host memory and
presents through shared-memory buffers only — no GPU device,
no GPU readback, no dma-buf
([ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md)). If the compositor is
slow to release buffers, the buffer pool reports no free slot and the daemon
drops that frame before doing CPU rendering instead of blocking the input loop.
The next dirty reactor step renders the newest coalesced candidate state.

The visible effect to the user is that the highlight briefly freezes
during the stall and then jumps to the correct candidate when the
compositor resumes; the committed text on the next text transaction
reflects whatever the user actually selected, not what was visually
highlighted at the moment of the stall.

## Configuration

Candidate-panel behavior is tuned through the display options of the platform
configuration file. Two of them affect the lifecycle directly: one enables or
disables the anchor probe, and the other bounds how long a positioned popup
waits for an anchor before the caret fallback runs or the update is dropped.
The accepted range is clamped at both ends, and a configured value below the
floor falls back to the built-in default. Exact option names, types, defaults,
and accepted ranges are in the
[Configuration Reference](../reference/configuration.md).

Theme options (panel colours for light and dark desktops) and font options
affect appearance but not the lifecycle.

## See Also

- [Panel Architecture](panel-architecture.md) — multi-owner arbitration, anchor probe overview
- [Frontend Graphics](frontend-graphics.md) — render pipeline and input correctness
- [Wayland Input Method Protocol](wayland-input-method.md) — protocol-layer events and serial chokepoint
- [Input-Method Session](input-method-session.md) — focus-in / focus-out / reactivation lifecycle
- [Panel Rendering Blueprint](../architecture/panel-rendering.md) — the current coordinator, scheduler, and render implementation
- [ADR-0040](../adr/0040-cpu-canvas-render-shm-buffers.md) — CPU canvas render and host-managed SHM buffers
- [ADR-0014](../adr/0014-canonical-panel-vocabulary.md) — Panel / Zone / popup vocabulary
- [ADR-0017](../adr/0017-positioned-ui-arbitration.md) — owner arbitration rules
- [ADR-0023](../adr/0023-panel-scheduler-state-machine.md) — panel scheduler history
