# ADR-0047: Headless Platform State Decoupling and CI Test Resilience

- **Status**: Accepted
- **Date**: 2026-09-11
- **Deciders**: Core Maintainers

## Context

`InputMethodFrontend` and `InputMethodState` in `crates/typio-host-platform` represent the
Wayland input-method protocol bridge. Previously, `InputMethodState` directly owned concrete
`wayland_client` proxy objects (`ZwpInputMethodV2`, `ZwpVirtualKeyboardV1`, `wl_surface`, etc.).
Consequently, constructing an `InputMethodState` or `InputMethodFrontend` required an active
Wayland display server (`$WAYLAND_DISPLAY`), and any attempt to test state transitions in a
headless environment (such as automated CI pipelines on GitHub Actions without a GUI compositor)
resulted in tests skipping execution (`eprintln!("skipping input_method state-helper test: no Wayland display"); return;`).

This left state transitions, candidate projections, pending key FIFO drains, and transaction
gating vulnerable to regression in continuous integration.

## Decision

1. **Encapsulate Wayland Transport Proxies**:
   Group all `wayland_client` protocol proxies into an internal `WaylandObjects` struct and
   place it behind `Option<WaylandObjects>` on `InputMethodState`.
2. **Provide Pure Headless Construction**:
   Expose `InputMethodState::new_headless()` and `InputMethodFrontend::new_headless()`.
   State accessors, candidate lists, composition sequences, focus facts, and pending key queues
   operate fully and deterministically without requiring a live Wayland connection.
3. **Graceful Degradation for Protocol Emitters**:
   Methods that emit Wayland requests (`forward_key`, `commit_protocol_state`,
   `text_transaction_and_flush`, `modifiers`) check for the presence of `WaylandObjects` and
   safely no-op in headless mode.
4. **Resilient Test Entrypoints**:
   Update `InputMethodFrontend::connect_test()` to fall back to `new_headless()` when no display
   is present, ensuring that tests such as `state_helpers_round_trip` and
   `headless_frontend_operations` run unconditionally and deterministically in CI.

## Alternatives considered

- **Spawn a nested headless Weston/Cage in CI**: Requires packaging full Wayland compositors
  and system dependencies into container runners, introducing flaky timeouts, GPU/DRM device
  mocking overhead, and slow test runs.
- **Mock traits over Wayland proxies**: Introduces excessive generic lifetime parameters and
  vtable indirection across all protocol dispatch routines for little benefit over an internal
  transport option.

## Consequences

- Positive: 100% test execution in headless environments and CI; zero skipped tests.
- Positive: Clear separation between pure state/focus facts and live Wayland socket operations.
- Trade-off: Internal proxy calls require an `if let Some(ref wayland) = self.wayland` check.
