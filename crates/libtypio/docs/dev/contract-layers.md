# Contract layers

The public headers under `include/typio/` are partitioned by audience and
stability promise. This file is the authoritative reference for which
header belongs in which layer and who is allowed to consume it.

| Layer        | Path                       | Audience                  |
|--------------|----------------------------|---------------------------|
| `abi/`       | `typio/abi/*.h`            | Engine worker implementations |
| `schema/`    | `typio/schema/*.h`         | User-facing config        |
| `runtime/`   | `typio/runtime/*.h`        | Hosts embedding libtypio  |

Cross-process control surfaces (UDS, D-Bus) are host concerns and live in
the relevant host repository — see [ADR-0007](../adr/0007-ipc-ownership-host-and-engine-backend-deferred.md).

## Rules

1. Engines consume **only** headers from `typio/abi/`. The convenience
   umbrella `typio/abi/abi.h` pulls in the whole engine ABI and is the
   recommended single include for native C engine implementations.

2. Hosts (`typio`, future platform hosts, and the
   control panel) consume anything from `typio/` and link `libtypio.so`.
   The convenience umbrella for hosts is `typio/typio.h`, which pulls in
   the engine ABI plus the runtime layer (`runtime/instance.h`,
   `runtime/registry.h`, `runtime/voice.h`) and the schema
   (`schema/config_schema.h`).

3. Cross-references between layers must be **explicit and absolute**:
   `#include "typio/<layer>/<file>.h"`. Never use a relative form — the
   path itself is part of the contract.

4. External processes (CLI, control panel, third-party clients) that need
   to talk to a running host depend on the host's own protocol contract,
   not on libtypio. They may still link the schema layer (`typio/schema/`)
   for shared config-key knowledge.

5. Engine discovery is the host's responsibility. The host implements
   `TypioPluginLoaderFunc` and registers each discovered worker via
   `typio_registry_register_engine_process`. Core bakes in no engine paths.

## Additive growth via `struct_size`

Caller-allocated structs (`TypioEngineInfo`, `TypioKeyEvent`,
`TypioComposition`) carry a `size_t struct_size` first field. The
framework reads only the fields the caller knew about, so optional
fields can be appended without breaking older engines.

## Memory ownership

A single deallocator family covers everything libtypio returns by value:

| Return shape | Free with |
|---|---|
| `char *` | `typio_free_string` |
| `char **` (with `size_t *count`) | `typio_free_string_array(list, count)` |
| `TypioEngineInfo *` | `typio_engine_info_free` |
| `TypioConfig *` | `typio_config_free` |

Strings allocated by the C caller must be released with the matching C
allocator. Never mix the two — on Windows, libtypio and the host may
link different CRTs.

The full rule is documented in `typio/abi/string.h`.

## Enforcement

Engine ABI compliance is enforced by code review.
