# Testing

This document is for contributors. It covers how to run and extend the
Typio test suite.

## Scope

The suite is the Cargo workspace suite. `crates/typio-host` covers the
shipping Rust daemon, subsystem ports, TIP framing, UDS IPC, engine
discovery, and headless daemon behavior. `crates/typio-core`,
`crates/typio-abi`, and `crates/typio-vet` cover the framework, shared ABI,
and engine conformance tooling. `crates/typioctl` covers the command-line
TIP/UDS client.

## Run Cargo Tests

Build or refresh the native renderer dependency first. These commands run from
the Typio repository root:

```bash
meson compile -C ../optics/build    # first setup uses --buildtype=debugoptimized
```

Run the full Rust suite:

```bash
cargo test -p typio-host
cargo test -p typio-core
cargo test -p typio-abi
cargo test -p typio-vet
cargo test -p typioctl
```

Run one test:

```bash
cargo test -p typio-host service::tests::hello_reports_protocol_and_capabilities
```

Run with output:

```bash
cargo test -p typio-host -- --nocapture
```

If `cargo test` reports an undefined `flux_*` symbol, Cargo loaded a stale
or system `libflux.so`. Rebuild `../optics`, then confirm
`FLUX_BUILD_DIR` points at the `debugoptimized` `../optics/build` tree.

## Cargo Coverage

| Area | Test surface |
|---|---|
| Daemon lifecycle | `app` unit tests, `tests/typio_daemon.rs` |
| TIP protocol and JSON-RPC framing | host `ipc` unit tests, `uds_server`, and `crates/typioctl/src/ipc.rs` |
| UDS server and IPC bus | `uds_server`, `ipc_bus`, `service` tests |
| Engine manifests and registration | `engine_loader` unit and integration tests |
| Wayland focus, key policy, repeat, candidate guard | `focus_controller`, `session_glue`, `keyboard_policy`, `keyboard::router`, `candidate_guard` tests |
| Panel policy and text UI state | `panel_scheduler`, `panel_coordinator`, `text_ui_state`, `preedit` tests |
| Tray and status state | `tray_menu`, `tray_sni`, `state_controller`, `language_display`, `icon_badge` tests |
| Runtime support | `config_watcher`, `resume_signal`, `health` tests |

## Add or Update Tests

Add or update tests when changing:

- Wayland lifecycle, key routing, repeat, or startup guard behavior
- runtime config reload, config-watch debounce, or event-loop scheduling
- voice service state transitions, reload deferral, or completion dispatch
- tray action handling or SNI serialization
- candidate Panel layout, rendering, or state classification
- focus-controller `reduce`, `diff`, or guard predicates
- TIP framing, UDS dispatch, or external-input parsing

Prefer small state-policy tests for Wayland behavior. Do not rely only on
manual compositor testing when a bug can be reduced to a helper or state
model.

## Style

- Use Rust tests for host behavior.
- Keep public API names in the style already used by the touched module.
- Prefer local helpers and direct data flow over broad abstractions.
- Document non-obvious behavior near complex state transitions.
- Keep generated protocol and renderer details behind narrow module
  boundaries.
