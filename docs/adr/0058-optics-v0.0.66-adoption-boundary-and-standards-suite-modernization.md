---
id: ADR-0058
title: "Optics v0.0.66 adoption boundary and standards suite modernization"
status: accepted
date: 2026-11-01
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0058: Optics v0.0.66 adoption boundary and standards suite modernization

- **Status**: Accepted
- **Date**: 2026-11-01
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: None

---

## Context

Upstream Optics progressed from `v0.0.62` through `v0.0.66`, introducing substantial architectural and developer-experience enhancements:

1. **Decoupled Host Canvas Recording (Optics ADR-0127)**: Replaced synchronous swapchain canvas drawing (`iris_paint_fn`) with decoupled host canvas recording (`iris_record_fn` & `flux_display_list`), moving vector recording entirely onto the CPU UI thread and eliminating Vulkan calls outside the presentation worker thread.
2. **Hardware-Locked Presentation Pacing**: Added DWM VBlank lockstep synchronization (`DwmFlush`) on Win32 and ProMotion 120Hz dynamic refresh discovery on Cocoa.
3. **Flux Non-Blocking Frame Acquire**: Added non-blocking frame begin semantics (`flux_frame_begin_desc.non_blocking`) and presenter priority elevation.
4. **Lens Compound Patterns & TextEdit Enhancements**: Added DataGrid, TreeView, and Menu compound patterns, textedit undo/redo ring buffers, and `submitted`/`tabbed` flags on `Response` for Return and Tab keystrokes.
5. **DocGov Decoupling & Standards Consolidation (Optics ADR-0125, ADR-0126)**: Abolished external `docgov` meta-framework tooling, removed nested `docs/governance/documentation/` mirror directories, flattened engineering standards into `docs/dev/standards/`, and streamlined AI directives.

Following the principles established in [ADR-0053](0053-optics-v0.0.62-adoption-boundary.md), upstream updates require deliberate evaluation: which designs match Typio's architectural premises, and which must be declined.

---

## Decision

1. **Atomically bump the workspace Optics dependencies to `v0.0.66`**:
   Advance the five workspace dependencies (`flux-sys`, `glyph-sys`, `iris`, `iris-sys`, `lens-sys`), `Cargo.lock`, and `OPTICS_PINNED_REF` in `.github/workflows/ci.yml` as one atomic unit to commit `c5ac994387f1728cbf715a50d1f5ce4d7d05627f`.

2. **Adopt Lens textfield Return submission in `typio-settings`**:
   Utilize `Frame::textfield_ex` and `Response.submitted` so that pressing Return inside a configuration textfield immediately applies the edited setting, eliminating unnecessary mouse interactions.

3. **Retain CPU-canvas `wl_shm` architecture for the Panel**:
   Decline GPU presentation pacing and swapchain recording for the candidate panel (`[INV-PANEL-13]`). The panel remains 100% CPU canvas to shared memory, driven by composition state events and backpressure (`[INV-PANEL-08]`).

4. **Modernize documentation governance and establish native standards**:
   Decouple completely from external `docgov` tooling and remove the nested `docs/governance/` hierarchy. Establish a flat, high-cohesion, native engineering standards suite directly in `docs/dev/standards/`:
   - `docs/dev/standards/README.md`: Central catalog & governance charter
   - `docs/dev/standards/architecture.md`: Workspace topology, crate layers, and architectural red-lines
   - `docs/dev/standards/invariants.md`: Canonical System Invariants registry and admissibility matrix
   - `docs/dev/standards/adr.md`: ADR qualification, schema, and lifecycle
   - `docs/dev/standards/rfc.md`: RFC deliberation protocol
   - `docs/dev/standards/taxonomy.md`: Diátaxis layout and Audience Firewall (`[INV-CORE-01]`)
   - `docs/dev/standards/style.md`: Link integrity and markdown conventions
   - `docs/dev/standards/workflow.md`: Verification gates and promotion protocol
   - `docs/dev/standards/postmortems.md`: Blameless incident postmortems

5. **Streamline `AGENTS.md` and verification tooling**:
   Strip external comment delimiters (`<!-- BEGIN DOCGOV DIRECTIVES -->`) and re-architect `AGENTS.md` around operational guardrails, mental models, and on-demand standards routing. Update `tools/check-docs.sh` to verify repository compliance without external mirror dependencies.

---

## Rationale

- **Atomic Upstream Pinning**: Pkg-config probes floor at `0.0.30`. Pinning dependencies, lockfiles, and CI environment variables together prevents silent linkage against stale system binaries.
- **Return Submission**: Immediate-mode text fields in desktop settings panels are expected to commit when pressing Enter. Leveraging `Response.submitted` aligns Typio Settings with desktop platform conventions.
- **Standards Flatness**: Sibling projects (`optics`, `sigil`, `tessera`) proved that flat `docs/dev/standards/` eliminates navigation friction while maintaining strict architectural invariants (`tools/check-docs.sh` and compiler mechanisms over external AST linters).

---

## Rejected Alternatives & Negative Knowledge

- **Retaining `docgov` mirror**: Rejected. The external `docgov` binary imported fragile comment boundary tags in `AGENTS.md` and redundant mirror hash checks that provided zero value to Typio's Rust runtime.
- **Adopting GPU swapchains for the Panel**: Rejected (`[INV-PANEL-13]`). Re-evaluated against Optics ADR-0127: GPU vector recording is valuable for windowed Iris applications, but introducing Vulkan swapchains to the Wayland input popup would add driver context creation lag and memory overhead for an ephemeral UI.

---

## Consequences

- All workspace crates build cleanly against canonical Optics `v0.0.66` and local dev worktrees.
- Typio Settings supports keyboard Return submission in configuration fields.
- Documentation standards are unified in `docs/dev/standards/`, and `tools/check-docs.sh` passes with zero errors.
- Epistemic lineage is self-contained within this repository.
