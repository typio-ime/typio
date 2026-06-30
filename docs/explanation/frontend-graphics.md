# Frontend Graphics

The host renders one floating UI: the **Panel**. Candidate composition,
engine/mode indicators, and voice status share the same input-popup
`wl_surface`; the active producer is selected by the Panel Coordinator before
rendering starts.

Rendering uses [flux](../../flux), a Vulkan canvas library, but the Panel does
not present through Vulkan WSI. The current path is:

```text
composition / indicator / voice state
  -> Panel Coordinator ownership and anchor policy
  -> FluxPanel sizing and layout
  -> flux_canvas draw commands on an offscreen flux_surface
  -> flux_surface_read_pixels
  -> host-managed wl_shm buffer
  -> wl_surface.attach + damage_buffer + commit
```

See [ADR-0040](../adr/0040-offscreen-render-shm-buffers.md) for the decision
that removed the Vulkan WSI swapchain from the Panel path.

## Rendering Boundaries

### Policy

Frontend policy lives in `crates/typio-host/src/panel_coordinator.rs` and the
event-loop caller. It decides:

- which UI owner may show;
- whether positioned UI has a trusted anchor;
- whether an anchor probe is needed;
- when a dirty candidate snapshot should be flushed.

This layer owns Wayland focus and input-method protocol facts. It does not draw.

### Render

`crates/typio-host/src/panel.rs` owns the flux objects:

- `flux_device`;
- offscreen `flux_surface`;
- `flux_canvas`;
- `flux_text`;
- transient arena and layout cache;
- optional `wp_viewport` crop state.

`FluxPanel::draw_candidates()` and `FluxPanel::draw_status_banner()` record the
actual canvas commands: transparent clear, rounded background, selection
highlight, text labels, and status text. They render into an offscreen image,
not directly into a Wayland surface.

### Present

`crates/typio-host/src/panel_shm.rs` owns the double-buffered SHM pool. The
rendered pixels are copied into a free `wl_buffer` and attached to the popup
surface. If the compositor has not released any buffer, the frame is dropped;
the event loop remains free to process input and later render the newest
coalesced state.

`wl_surface.frame` callbacks are retained only as pacing hints. A missing
callback no longer freezes rendering indefinitely; the soft gate wakes on a
deadline and allows a timer-paced submit.

## Flux Dependency Boundary

The host does not call Vulkan directly. Its graphics dependency is the small
flux surface/canvas/text API:

| Concept | Current use |
|---|---|
| `flux_device` | Process-local GPU device for Panel rendering. |
| `flux_surface` | Offscreen render target (`vk_surface_khr = NULL`). |
| `flux_canvas` | Immediate-mode draw target for fills and rounded rectangles. |
| `flux_text` | Text measurement and glyph drawing. |
| `flux_surface_read_pixels` | GPU-to-CPU readback before SHM attach. |

Porting to another canvas backend would require replacing `FluxPanel`'s
surface/canvas/text calls and the readback step. Ownership policy, anchor
handling, key routing, and engine state do not depend on flux.

## Input Correctness

Rendering is downstream of input. A skipped or delayed frame can make the
visible highlight lag briefly, but it does not change which candidate is
selected or committed. Key events remain ordered on the Wayland event loop, and
the next successful Panel frame consumes the latest coalesced composition
snapshot.

## See Also

- [Panel Architecture](panel-architecture.md) — ownership and anchor policy.
- [Candidate Panel Behavior](candidate-panel-behavior.md) — user-visible Panel
  lifecycle.
- [Vulkan and Flux Rendering](vulkan-flux-rendering.md) — flux-specific
  rendering details.
- [ADR-0040](../adr/0040-offscreen-render-shm-buffers.md) — offscreen render
  and host-managed SHM buffers.
- [ADR-0014](../adr/0014-canonical-panel-vocabulary.md) — canonical Panel
  terminology.
