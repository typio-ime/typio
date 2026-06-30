# Typio Documentation

Typio is a native Linux input method framework. Core business logic lives in
the Rust `libtypio` library; `typio` provides the Wayland host, and
engines run as separate engine processes.

## Start by role

If you are building something against libtypio, start here — these pages walk you through the existing docs in the right order for your role.

- **[Host Integrator Path](host-integrator.md)** — building `typio`, an alternate platform daemon, or anything else that embeds `libtypio`.
- **[Engine Author Path](engine-author.md)** — building a keyboard or voice engine executable and manifest.

## Sections

- **[Tutorials](tutorials/)** — Step-by-step lessons. Start here if you are new to Typio.
  - [Getting Started](tutorials/01-getting-started.md) — Build, test, and run Typio for the first time.
- **[How-to Guides](how-to/)** — Task-oriented recipes for specific goals.
  - [Install Typio](how-to/install.md)
  - [Configure Typio](how-to/configure.md)
  - [Create a Custom Keyboard Engine](how-to/create-custom-keyboard-engine.md)
  - [Create a Custom Voice Engine](how-to/create-custom-voice-engine.md)
  - [Integrate a Keyboard Engine](how-to/integrate-keyboard-engine.md)
  - [Integrate a Voice Engine](how-to/integrate-voice-engine.md)
- **[Reference](reference/)** — Lookup-oriented API, config, and protocol documentation.
  - [Host ABI Reference](reference/host-abi/) — for hosts linking `libtypio.so`
  - [Engine Reference](reference/engine/) — for engine authors
  - [CLI Reference](reference/cli.md)
  - [Configuration Reference](reference/configuration.md)
  - [Engine Reference](reference/engines.md) — Keyboard and voice engine configs, capabilities, and ABI
- **[Explanation](explanation/)** — Understanding-oriented design documents.
  - [Architecture Overview](explanation/architecture-overview.md)
  - [Config & Runtime Ownership](explanation/config-runtime-ownership.md)
  - [Configuration System](explanation/configuration-system.md)
  - [Voice Input Architecture](explanation/voice-input.md)
- **[Architecture Decisions](adr/)** — Immutable records of past design decisions.
- **[Developer Documentation](dev/)** — Contributor-oriented docs.
  - [Developer Setup](dev/setup.md)
  - [Testing](dev/testing.md)
  - [Code Style](dev/code-style.md)
  - [Project Layout](dev/project-layout.md)

## Quick Links

- [README](../README.md) — Project pitch and 30-second quick start
- [CHANGELOG](../CHANGELOG.md) — Version history
- [Contributing](../CONTRIBUTING.md) — How to contribute
