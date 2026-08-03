# Typio Documentation

The Linux/Wayland Typio workspace. This repository contains the `typio` daemon,
the `typioctl` client, the `typio-settings` graphical application, the
runtime and typed engine-contract crates, the candidate Panel renderer, the UDS
control surface (TIP v1), system tray integration, and voice capture plumbing.

## Sections

- **[How-to Guides](how-to/)** — Task-oriented recipes for specific goals.
  - [How to Package for Distribution](how-to/package-for-distribution.md)
  - [How to Configure Typio Graphically](how-to/configure-graphically.md)
  - [Troubleshooting](how-to/troubleshooting.md)
  - [How to Diagnose Candidate-Switching Lag](how-to/diagnose-candidate-lag.md)
- **[Reference](reference/)** — Lookup-oriented API, config, and protocol documentation.
  - [Glossary](reference/glossary.md) — Canonical project terms with definitions and sources
  - [IPC Protocol Reference](reference/ipc-protocol.md) — TIP v1 (UDS + JSON-RPC)
  - [Configuration Reference](reference/configuration.md) — `core.toml` and `platform.toml` keys, reload behaviour, and file structure
  - [Engine Discovery Reference](reference/engine-discovery.md) — search path, file-name rules, icons
  - [Interface Stability Reference](reference/stability.md) — stability tiers for every external interface
- **[Explanation](explanation/)** — Understanding-oriented design documents.
  - [Project Scope: Host, Runtime, Engine Protocol, Vet, and Clients](explanation/project-scope.md)
  - [Wayland Input Method Protocol](explanation/wayland-input-method.md)
  - [Input-Method Session](explanation/input-method-session.md) — Disambiguates the three layers of "session"
  - [Focus Controller](explanation/focus-controller.md) — Derived-state, idempotent-diff lifecycle model
  - [Panel Architecture](explanation/panel-architecture.md)
  - [Candidate Panel Behavior](explanation/candidate-panel-behavior.md) — UI-level lifecycle: show/hide, anchor, retry, input → visible effect
  - [Frontend Graphics](explanation/frontend-graphics.md)
  - [Input-Method Session](explanation/input-method-session.md) — three layers of session, build-up chain, and lifecycle rules
  - [Event Loop Scheduling](explanation/event-loop-scheduling.md) — CPU render bounds, D-Bus dispatch, config reload, and poll deadlines
  - [Performance & Idle-Power Strategy](explanation/performance-strategy.md) — event-driven idle, zero wakeups, deadline folding, and how to measure
  - [Control Surfaces](explanation/control-surfaces.md)
  - [Security Model](explanation/security-model.md) — trust boundaries, engine trust, and the sandboxing path
- **[Developer Documentation](dev/)** — Contributor-oriented docs.
  - [Developer Setup](dev/setup.md)
  - [Testing](dev/testing.md)
  - [Code Style](dev/code-style.md)
  - [Panel Appearance](dev/panel-appearance.md)
  - [Documentation Governance](dev/documentation/)

## Quick Links

- [README](../README.md) — Project pitch and quick start
- [Contributing](../CONTRIBUTING.md) — How to contribute
