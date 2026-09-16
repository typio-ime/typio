# ADR-0007: IPC ownership — control surface to host, engine backend deferred

- **Status**: Accepted
- **Date**: 2026-05-28
- **Deciders**: Project maintainers

## Context

Two unrelated kinds of "IPC" had crept into `libtypio`:

1. **Daemon control IPC** — the UDS / D-Bus surface that lets `typioctl`, `typio-settings`, and third-party tools talk to a running host. The wire protocol's *server* implementation already lives in `typio-wayland`, but the shared protocol constants (`org.typio.InputMethod1`, property names, method names) lived in `include/typio/ipc/dbus_protocol.h`, and the wire format was documented in `docs/reference/ipc-protocol.md` and `docs/reference/dbus-interface.md`.
2. **Engine IPC backend** — `EngineBackend::Ipc` and `IpcEngineProxy`, an out-of-process engine transport introduced by [ADR-0005](0005-internal-engine-backend-abstraction.md). The variant existed but every method was a `todo!()` or returned `EngineError::Transport("not yet implemented")`.

Both situations were architecturally muddled:

- The control IPC is a *host* surface. [ADR-0004](0004-platform-neutral-core-host-loading.md) declared libtypio platform-neutral: it knows about engines, configs, and input contexts, but it does not know about Wayland, D-Bus, or sockets. Carrying D-Bus interface names in libtypio's public headers contradicted that. A future X11 or evdev host would not use the same protocol, yet would inherit irrelevant constants through `typio/typio.h`.
- The engine IPC backend was unreachable code. No engine plugin ships an IPC transport; nothing in the registry or `c_api/` constructs `EngineBackend::Ipc`. The stub existed to preserve the option of adding IPC later, but stubs that never get called rot — the `Request`/`Response` enums and framing would need redesign by the time any real consumer arrives.

## Decision

1. **Move the control-IPC protocol contract to `typio-wayland`.**
   - Delete `include/typio/ipc/dbus_protocol.h` from libtypio; remove the `typio/ipc/` layer from `typio/typio.h` and from [Contract Layers](../dev/contract-layers.md).
   - Delete `docs/reference/ipc-protocol.md`, `docs/reference/dbus-interface.md`, and `docs/how-to/communicate-over-uds.md` from libtypio. The host-side protocol — including the constants header — will be re-published in `typio-wayland` (and consumed by `typioctl` and `typio-settings` from there).
2. **Delete the IPC engine backend skeleton.**
   - Remove `src/core/engine/backend/ipc/` and the `EngineBackend::Ipc` enum variant.
   - The `EngineBackend` abstraction remains — `FfiBackend` is now the only variant. If and when a real out-of-process engine arrives, an `EngineBackend::Ipc` variant can be added in a focused change instead of carried as dead weight indefinitely.

## Alternatives considered

- **Keep `dbus_protocol.h` in libtypio as a "shared constants" header.** Rejected: it crosses the platform-neutrality boundary, and the only callers (`typioctl`, `typio-settings`) already need to depend on `typio-wayland` for behaviour, not just constants. Duplicating ~40 lines of `#define`s across consumers, or publishing a tiny `typio-wayland-protocol` companion crate, is cheaper than the architectural smell.
- **Keep the IPC backend skeleton.** Rejected: it was unreachable, and the wire protocol it sketched (`Request::FocusIn { ctx_id: 0 }`) has not been validated against any real consumer. A skeleton that has not been load-bearing for any caller does not stabilize a future design — it locks in a guess.
- **Move the backend skeleton to a feature-gated module.** Rejected: same problem as deletion (no consumer keeps it honest) with the added cost of feature-flag complexity in `Cargo.toml` and CI.

## Consequences

- Positive: libtypio's public surface no longer mentions D-Bus, UDS, or sockets. The platform-neutrality claim in ADR-0004 holds without an asterisk.
- Positive: dead code is gone. `EngineBackend` collapses to a single-variant enum until it has an actual second transport, at which point the design is informed by a real caller.
- Positive: hosts that are not `typio-wayland` (future X11, evdev, Windows) do not inherit a D-Bus naming scheme they do not implement.
- Trade-off: consumers of the D-Bus / UDS protocol constants (`typioctl`, `typio-settings`) must either depend on `typio-wayland`'s published header or carry their own copy of the constants. Both options are tolerable; the published-header path is recommended.
- Trade-off: adding out-of-process engines later requires re-introducing `EngineBackend::Ipc` and the proxy. Since the registry only sees `EngineBackend`, this remains a one-trait-impl change — no registry refactor — exactly the property [ADR-0005](0005-internal-engine-backend-abstraction.md) preserved.

## Related

- [ADR-0002: C ABI as the only public interface](0002-c-abi-as-the-only-public-interface.md) — defines what counts as a libtypio public surface.
- [ADR-0004: Platform-neutral core, host-owned loading](0004-platform-neutral-core-host-loading.md) — the principle this ADR enforces more strictly.
- [ADR-0005: Internal engine backend abstraction](0005-internal-engine-backend-abstraction.md) — defined `EngineBackend` with an FFI and an IPC variant; this ADR removes the IPC variant for now while leaving the abstraction shape intact.
