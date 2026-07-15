# Engine ABI Stability

The contract between a native engine implementation and the engine runtime is
the **engine ABI**. This page is the single source of truth for how that
contract is versioned and negotiated.

## The version pair

The ABI is identified by a `(major, minor)` pair, defined once in
[`include/typio/abi/version.h`](../../include/typio/abi/version.h) and mirrored
in the `typio-abi` crate:

| Constant | Meaning |
|----------|---------|
| `TYPIO_ENGINE_ABI_MAJOR` | Incremented on any incompatible change (struct layout, callback signature, or semantics). |
| `TYPIO_ENGINE_ABI_MINOR` | Incremented on backward-compatible additions (e.g. a new optional callback at the end of a vtable). |

Pre-1.0, `major` is `0` and the pair may move freely between releases; engines
must rebuild against the headers they target.

## How the version is carried

Every native C engine exports a single out-of-band entry point:

```c
const TypioAbiVersion *typio_engine_abi_version(void);
```

The `TYPIO_KEYBOARD_ENGINE_DEFINE` / `TYPIO_VOICE_ENGINE_DEFINE` macros emit it
automatically, reporting the `MAJOR`/`MINOR` the engine was compiled against.
This export gates engine metadata and vtable compatibility. The former
`TypioEngineInfo.struct_size` no longer exists.

## The negotiation algorithm

A direct worker links `typio_engine_abi_version` into the executable; the
shared harness validates it before schema discovery, metadata access, or
engine construction. A compatibility worker may resolve it with `dlsym`
inside the worker process before calling any other engine symbol. The result
is passed to:

```c
bool typio_engine_abi_check(const TypioAbiVersion *reported);
```

A native engine is accepted when **all** hold:

- the reported version is non-NULL,
- `major == TYPIO_ENGINE_ABI_MAJOR` (exact major match), and
- `minor <= TYPIO_ENGINE_ABI_MINOR` (the runtime understands every engine ABI
  feature the engine can rely on).

An engine built against a newer minor than the runtime is rejected: it may
depend on additions the runtime does not provide. A major mismatch is always
rejected.

## Struct-size compatibility

Caller-allocated data structures such as `TypioKeyEvent` and
`TypioComposition` still carry a `struct_size` first field. That field is for
append-only data payload evolution after the engine has already passed ABI
version negotiation. Readers honour only the fields the writer's size covers.

## What this means for engine authors

- Use the `TYPIO_*_ENGINE_DEFINE` macros; they wire up the version export for
  you. If you hand-roll your entry points, you must define
  `typio_engine_abi_version` yourself.
- Rebuild against the headers for the runtime you target; do not assume an
  engine built against one minor works on an older runtime.
- Confirm native C engine builds export `typio_engine_abi_version` when they
  rely on symbol lookup, or link the symbol into the worker executable when
  using a direct worker harness.
