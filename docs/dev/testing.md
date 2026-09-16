# Testing

This document is for contributors. It covers the automated suites, the
environment they require, how they run in CI, and when a change owes a test.
End-to-end user outcomes are verified separately in [Acceptance](acceptance.md).

Every command here must run deterministically from a clean checkout: no hidden
local fixture, no unstated environment variable, no unversioned download
([INV-VAL-02](../governance/documentation/core/invariants.md)). If a command
below needs something the matrix does not list, that is a bug in this page.

## 1. Suite Commands

Run these from the repository root.

| Suite | Command | What it covers |
| :--- | :--- | :--- |
| Whole workspace | `cargo test --workspace` | Every unit and integration test target in the workspace |
| One crate | `cargo test -p typio-daemon` | The daemon: Wayland lifecycle, TIP framing and UDS dispatch, engine loading, panel policy |
| Runtime | `cargo test -p typio-runtime` | Engine registry, configuration schema, process backend |
| Engine contract | `cargo test -p typio-engine-protocol -p typio-engine-manifest` | Frame and message encoding, manifest parsing and value rules |
| Engine conformance | `cargo test -p typio-engine-check` | Black-box worker scenarios driven through the real process boundary |
| Clients | `cargo test -p typio-client -p typio-control -p typio-settings` | TIP client framing and events, CLI dispatch, settings model and platform-config persistence |
| One test | `cargo test -p typio-daemon service::tests::hello_reports_protocol_and_capabilities` | A single test by path |
| With output | `cargo test -p typio-daemon -- --nocapture` | Same suite, without capturing `stdout` |
| Formatting gate | `cargo fmt -- --check` | Formatting drift |
| Lint gate | `cargo clippy --workspace --all-targets -- -D warnings` | Warnings as errors |
| Dependency audit | `cargo audit` | Known-vulnerable locked dependencies |

Build the native renderer dependency before the first run; see
[Developer Setup](setup.md). Without it, `cargo test` fails to link the
`flux`-backed crates rather than failing a test.

## 2. Environment and Fixture Matrix

| Variable | Needed by | Value in a local tree |
| :--- | :--- | :--- |
| `FLUX_BUILD_DIR` | Daemon and Panel crates linking the native flux library | The sibling Meson build tree, e.g. `$PWD/../optics/build` |
| `FLUX_SOURCE_DIR` | Bindings generated from flux headers | The sibling flux source, e.g. `$PWD/../optics/libs/flux` |
| `LENS_BUILD_DIR`, `LENS_SOURCE_DIR` | Settings application (Lens UI toolkit) | The sibling Optics build tree and source root |
| `IRIS_BUILD_DIR`, `IRIS_SOURCE_DIR` | Settings application (Iris windowing) | The sibling Optics build tree and source root |
| `LD_LIBRARY_PATH` | Crates that load a built shared library at test time | Must include `target/debug` and the Optics build tree |
| `RUST_LOG` | Tracing output only | Optional; never required for a test to pass |
| `TYPIO_ENGINE_PATH`, `--engine-dir` | Manual runs and development only | Optional; the shipped default is the system engine directory |

**Test data and fixtures.** Tests that need an engine use in-process or
spawned worker fixtures under the owning crate; tests that need a configuration
file write it into a temporary directory created by the test itself. No test
reads the developer's real configuration or engine directory.

**Isolation and teardown.** Tests that create temporary directories, sockets, or
child processes must remove or reap them when they finish. A test that leaves a
socket behind will break the next run on the same machine, and CI is not
exempt from that rule.

**What tests must not require.** A live Wayland compositor, a running daemon
from a previous command, a network download, or a pre-populated user
configuration. Headless paths exist precisely so that lifecycle and policy tests
run without a display.

## 3. Continuous Integration

Every job runs on `ubuntu-24.04` and builds a pinned Optics revision
(`OPTICS_PINNED_REF` at workflow level) before touching Cargo, because the
native flux library is a link-time dependency.

| Job | Gate | Blocks a pull request |
| :--- | :--- | :--- |
| Check, Format, and Lint | `cargo fmt -- --check`; `cargo clippy --workspace --all-targets -- -D warnings` | Yes |
| Cargo build and test | `cargo build --workspace`; `cargo test --workspace` | Yes |
| Documentation governance | `tools/check-docs.sh` | Yes |
| RustSec audit | `cargo audit` | Yes |
| Valgrind leak gate | Leak-sensitive suites (`icon_badge`, `text_raster`, `panel`) under `--errors-for-leak-kinds=definite`, with `tools/valgrind-leak-gate.supp` | Yes |

The leak gate suppresses only libfontconfig's process-lifetime pattern cache. A
real leak on the Typio side fails the gate.

When a test runner, environment variable, or CI suite list changes, this page
changes in the same pull request.

## 4. Coverage Map

| Area | Test surface |
| :--- | :--- |
| Daemon lifecycle | `app` unit tests, `tests/typio_daemon.rs` |
| TIP protocol and JSON-RPC framing | host `ipc` unit tests, `uds_server`, `typio-client` tests |
| Settings config persistence and TIP model decoding | `typio-settings` unit tests |
| UDS server and IPC bus | `uds_server`, `ipc_bus`, `service` tests |
| Engine manifests and registration | `engine_loader` unit and integration tests |
| Engine wire framing and typed messages | `typio-engine-protocol` unit tests |
| Engine process conformance | `typio-engine-check` black-box worker scenarios |
| Wayland focus, ordered keyboard transport (including 2,048 shortcut dispatch partitions), key policy, repeat, candidate guard | `focus_controller`, `session_glue`, `keyboard_policy`, `keyboard::router`, `candidate_guard` tests |
| Panel policy and text UI state | `panel_scheduler`, `panel_coordinator`, `text_ui_state`, `preedit` tests |
| Tray and status state | `tray_menu`, `tray_sni`, `state_controller`, `language_display`, `icon_badge` tests |
| Runtime support | `config_watcher`, `resume_signal`, `health` tests |

## 5. When a Change Owes a Test

Add or update tests when changing:

- Wayland lifecycle, key routing, key repeat, or the startup guard
- runtime config reload, config-watch debounce, or event-loop scheduling
- voice service state transitions, reload deferral, or completion dispatch
- tray action handling or StatusNotifierItem serialization
- candidate Panel layout, rendering, or schedule-state classification
- focus-controller `reduce`, `diff`, or guard predicates
- TIP framing, UDS dispatch, or external-input parsing
- IPC client behavior, CLI command parsing and dispatch, or the public CLI
  interface and output format of `typioctl`

Prefer a small state-policy test over manual compositor testing whenever a bug
can be reduced to a helper or a state model.

## Style

- Use Rust tests for host behavior.
- Keep public API names in the style already used by the touched module.
- Prefer local helpers and direct data flow over broad abstractions.
- Document non-obvious behavior near complex state transitions.
- Keep generated protocol and renderer details behind narrow module boundaries.

## See also

- [Acceptance](acceptance.md) — user journeys verified by hand before a release
- [Developer Setup](setup.md) — native prerequisites and the Optics build tree
- [Optics Dev Worktree](optics-dev-worktree.md) — testing against a local Optics worktree
