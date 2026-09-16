# Typio Documentation

Typio is a Wayland-native input-method host for Linux. This repository contains
the `typio` daemon, the `typioctl` control client, the `typio-settings`
application, the runtime and typed engine-contract crates, the candidate Panel
renderer, the TIP control surface, system tray integration, and voice capture.

The documentation below is organised by what you are trying to do. Every
directory has a charter page describing its scope — start there when you are not
sure where something belongs.

## Learn

- [Tutorials](tutorials/index.md) — guided walkthroughs from a cold start
  - [Your First Typio Session](tutorials/getting-started.md) — install, start, and type with an engine

## Do

- [How-to Guides](how-to/index.md) — recipes for a specific goal
  - [How to Package for Distribution](how-to/package-for-distribution.md)
  - [How to Configure Typio Graphically](how-to/configure-graphically.md)
  - [How to Diagnose Candidate-Switching Lag](how-to/diagnose-candidate-lag.md)
  - [How to Communicate with Typio over UDS](how-to/communicate-over-uds.md)
  - [How to Write an Engine](how-to/write-an-engine.md)
  - [Troubleshooting](how-to/troubleshooting.md)

## Look up

- [Reference](reference/index.md) — exact commands, keys, and wire formats
  - [Command-Line Interface](reference/cli.md) — `typio`, `typioctl`, `typio-settings`
  - [Configuration](reference/configuration.md) — `core.toml` and `platform.toml`
  - [IPC Protocol (TIP v3)](reference/ipc-protocol.md) — the control socket
  - [Engine Protocol](reference/engine-protocol.md) — the manifest-declared worker contract
  - [Engine Discovery](reference/engine-discovery.md) — where manifests are found
  - [Interface Stability](reference/stability.md) — the tier of every external interface
  - [Glossary](reference/glossary.md) — canonical project terms

## Understand

- [Explanation](explanation/index.md) — why the system is shaped this way.
  Covers the input-method session, the focus controller, Panel architecture and
  rendering, event-loop scheduling, performance strategy, stall containment, the
  control surfaces, the configuration system, the security model, and voice input.

## Contribute

- [Architecture Blueprints](architecture/index.md) — how each subsystem works today
- [Architecture Decision Records](adr/index.md) — the immutable decisions behind it
- [Repository Governance](governance/index.md) — charters, gates, and the release process
- [Developer Documentation](dev/index.md), behind the contributor firewall
  ([INV-CORE-01](governance/documentation/core/invariants.md)): setup, module map,
  testing, acceptance, code style, and cross-repository development
- [CONTRIBUTING.md](../CONTRIBUTING.md) — the contribution entry point
- [README.md](../README.md) — project pitch and quick start
