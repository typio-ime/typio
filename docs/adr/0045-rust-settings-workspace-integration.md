# ADR-0045: Rust Settings Workspace Integration

- **Status**: Accepted
- **Date**: 2026-07-13
- **Supersedes**: ADR-0035 decision D5 for `typio-settings`; ADR-0039's settings-panel deferral

## Context

The settings application remained a sibling Meson/C repository after the host,
framework, ABI, vet tool, and CLI moved into one Cargo workspace. Its custom
Wayland/Vulkan shell duplicated application plumbing now provided by the Iris
and Lens Rust bindings in the sibling `optics` checkout. It also mixed direct
framework access with daemon IPC, which made a single settings screen depend on
two control paths and allowed its key vocabulary to drift from TIP.

Settings changes are tightly coupled to daemon protocol and schema changes in
the same way CLI changes are. Keeping the application in a separate repository
therefore no longer provides an independent release boundary; it prevents the
client and server sides of a protocol change from landing atomically.

The Panel's `platform.toml` is different from schema-backed core configuration.
It is owned by the Wayland frontend and is not exposed through TIP, so a GUI
still needs one narrow file-persistence path.

## Decision

Add `crates/typio-settings` to the main Cargo workspace as a Rust application.

- Iris owns the native application window and event loop. Lens builds the
  immediate-mode settings interface, and Flux remains the rendering substrate.
  The application consumes the safe Rust bindings from `../optics`; it does not
  import the old C window/platform implementation.
- Add `crates/typio-client` as the shared synchronous TIP client. Both
  `typioctl` and `typio-settings` use its length-prefixed JSON-RPC framing,
  response validation, socket discovery, and event subscription.
- Read and mutate schema-backed configuration, engine state, language state,
  and engine commands only through TIP. The UI is generated from `config.list`
  and `engine.describe` instead of maintaining its own property table.
- Edit only `platform.toml` directly. Use a comment-preserving TOML document,
  update the `[display]` keys owned by the UI, retain unknown keys, and replace
  the file atomically.
- Subscribe to daemon events and refresh the schema snapshot when state changes.
  Keep the Appearance page usable when the daemon is unavailable.
- Install the settings binary, freedesktop desktop entry, AppStream metadata,
  and application icon through the workspace's existing `xtask` installer.

## Alternatives Considered

- **Move the C/Meson project into this repository unchanged.** Rejected because
  it would preserve the duplicate platform shell and create a second build
  system for an application whose supported graphics stack already has Rust
  bindings.
- **Link the GUI directly to typio-core.** Rejected because a settings client
  would then hold config state independently from the running daemon. TIP is the
  canonical external control surface and supplies runtime state plus events.
- **Expose `platform.toml` through new TIP methods first.** Deferred. The file
  is frontend-specific, and a comment-preserving atomic editor is a contained
  boundary. A future protocol may absorb it if another client needs the same
  operations.
- **Adopt a separate desktop widget toolkit.** Rejected because Iris/Lens/Flux
  is the project-specified graphics stack and is already built as part of the
  sibling optics monorepo.

## Consequences

- Positive: settings protocol and schema changes can land atomically with the
  daemon, shared client, CLI, tests, packaging, and documentation.
- Positive: the old C platform shell and direct libtypio control path are not
  carried forward.
- Positive: TIP framing has one client implementation and gains direct tests
  for subscriptions and partial event delivery.
- Trade-off: building and packaging the GUI requires the Iris and Lens native
  libraries in addition to Flux.
- Trade-off: `platform.toml` remains a deliberate second persistence path until
  the daemon exposes frontend-owned display settings over TIP.
