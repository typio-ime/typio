# ADR-0005: Internal Engine Backend Abstraction

- **Status**: Accepted
- **Date**: 2026-05-28
- **Deciders**: Project maintainers

## Context

The public C ABI for plugin engines is fixed by [ADR-0003](0003-plugin-engine-abi-dual-category.md): C vtables, dual-category slots, factory entry points. Inside `libtypio`, the question is how to represent registered engines so that:

1. **Engine plugins loaded over different transports look the same to the registry.** In-process FFI engines (the common case) and any future out-of-process IPC engines (sandboxed voice models, etc.) should both register into the same slots through the same trait.
2. **Reasoning happens through safe Rust, not raw pointers.** The registry must not carry `*mut TypioEngine` around the core; that style produced multi-layer adapter towers that defeated debugging.
3. **Auto-unload is a registry-level policy.** Idle timeouts for heavy engines (voice models) belong to the registry, not to ad-hoc threads owned by each engine.

## Decision

- **Pure Rust traits are the sole internal interface**: `Engine`, `KeyboardEngine`, `VoiceEngine`. The registry holds `Box<dyn EngineBackend>` and calls into engines through trait objects.
- **`EngineBackend` is the single transport abstraction.** `FfiBackend` (in-process plugin loaded by the host per [ADR-0004](0004-platform-neutral-core-host-loading.md)) is the primary implementation. The trait leaves room for an `IpcBackend` without forcing one to exist.
- **`TypioRegistry` replaces the historical engine-manager**, managing slots with declarative `IdlePolicy::{KeepAlive, DeactivateAfter, UnloadAfter}` and automatic unload.
- **The C ABI surface is a thin wrapper** in `src/c_api/` that translates C calls into operations on the Rust registry. The C vtable types from [ADR-0003](0003-plugin-engine-abi-dual-category.md) live only at this boundary; internal core code never sees them.

### Architecture

```text
┌─────────────────────────────────────────────┐
│  c_api/      thin C boundary (ADR-0002/3)   │
├─────────────────────────────────────────────┤
│  core::registry::TypioRegistry              │
│    slots, active_keyboard, active_voice,    │
│    UnloadScheduler                          │
├─────────────────────────────────────────────┤
│  core::engine traits                        │
│    Engine, KeyboardEngine, VoiceEngine      │
├─────────────────────────────────────────────┤
│  core::engine::backend::EngineBackend       │
│    FfiBackend (host-loaded plugin)          │
└─────────────────────────────────────────────┘
```

### Key principles

1. **Registry never touches raw pointers.** All engine operations go through `backend.as_engine()` → `&mut dyn Engine`.
2. **Auto-unload is a registry policy, not a backend detail.** `IdlePolicy` is evaluated by the registry; the backend only implements `destroy()`.
3. **No C vtables inside core.** Vtables exist only in `src/c_api/`. Internal code reasons in Rust types.
4. **Backend preference is a declaration, not a runtime branch.** Each engine declares its preference (keyboard defaults to FFI; voice may prefer IPC if available), and the host picks an appropriate backend at registration time.

## Alternatives considered

- **Keep C vtables as the internal model and wrap any IPC backend as a pseudo-C-plugin.** Rejected: would require an IPC backend to masquerade as a C vtable, then be adapted back to Rust traits — a three-layer adapter tower.
- **Make IPC the default for all engines.** Rejected: keyboard input is latency-sensitive (~1–10 ms IPC overhead is perceptible at 10 CPS). Defaults must favor in-process for the typing path.
- **Branch on `backend.is_ipc()` throughout the registry.** Rejected: bakes transport identity into every call site, defeating the abstraction.

## Consequences

- Positive: a single coherent abstraction. Adding a new transport (WebAssembly, gRPC, …) is one trait impl, not a registry refactor.
- Positive: auto-unload applies uniformly to all engine types and transports.
- Positive: Rust engine authors write plain `impl KeyboardEngine for MyEngine`. No `extern "C"` factories required inside core; the FFI shell exists only at the published ABI boundary.
- Trade-off: a future IPC backend requires a wire protocol to be designed and stabilized. Until that exists, only `FfiBackend` is usable — which is fine for the current scope.

## Related

- [ADR-0002: C ABI as the only public interface](0002-c-abi-as-the-only-public-interface.md) — the published ABI that `c_api/` translates to/from
- [ADR-0003: Plugin engine ABI — dual-category slots](0003-plugin-engine-abi-dual-category.md) — the dual-category concept preserved inside the registry
- [ADR-0004: Platform-neutral core, host-owned loading](0004-platform-neutral-core-host-loading.md) — host loads the plugin and constructs the FFI backend
