# Engine Protocol Reference

## Identity

| Item | Value |
|------|-------|
| Name | Typio Engine Protocol |
| Manifest value | `typio-engine-protocol` |
| Version | `1.0` |
| Magic | `TYEP` |
| Rust contract | [`typio-engine-protocol`](../../../typio-engine-protocol/README.md) |
| Runtime backend | `typio_core::core::engine::backend::ProcessBackend` |

## Transport

| Item | Value |
|------|-------|
| Process model | Host starts one engine executable on demand |
| Protocol fd | `3` |
| Environment | `TYPIO_ENGINE_PROTOCOL=1.0`, `TYPIO_ENGINE_FD=3` |
| Standard input | Closed to engine protocol traffic |
| Standard output | Logs only |
| Standard error | Logs only |
| Host registration | `EngineRegistry::register` with `ProcessBackend` |

## Frame Header

| Offset | Size | Type | Value |
|--------|------|------|-------|
| 0 | 4 | `u32` | `TYEP` magic, network byte order |
| 4 | 2 | `u16` | Major version |
| 6 | 2 | `u16` | Minor version |
| 8 | 4 | `u32` | Message type |
| 12 | 4 | `u32` | Flags, reserved |
| 16 | 8 | `u64` | Request id |
| 24 | 4 | `u32` | Payload length |

## Message Types

| Value | Name | Direction |
|-------|------|-----------|
| 1 | `EngineHello` | Engine to host |
| 2 | `HostHello` | Host to engine |
| 3 | `Request` | Host to engine |
| 4 | `Response` | Engine to host |
| 5 | `Event` | Engine to host |
| 6 | `Error` | Either direction |

## Limits

| Item | Value |
|------|-------|
| Maximum payload | 8 MiB |
| Major version mismatch | Transport error |
| Response request id | Must echo request id |

## EngineHello Payload

```text
protocol	1.0
engine	<name>
type	<keyboard|voice>
SCHEMA	<key-hex>	<type>	<default>	<label-hex>	<section-hex>	<min>	<max>	<step>	<options>	<runtime-key-hex>
```

The first three records are mandatory. `SCHEMA` may occur zero or more times.
Schema type is `0` (string), `1` (int), `2` (bool), or `3` (float); string
values are hexadecimal UTF-8; options are a
comma-separated list of hexadecimal strings. Numeric defaults and range hints
are decimal text. Every key must be under `engines.<engine-name>.`.

## HostHello Payload

```text
protocol	1.0
engine	<name>
type	<keyboard|voice>
config-dir	<hex-encoded UTF-8 path>
data-dir	<hex-encoded UTF-8 path>
state-dir	<hex-encoded UTF-8 path>
```

The path records carry the daemon's exact runtime roots, including command-line
overrides. Workers use them instead of reconstructing paths from ambient XDG
variables. Engine-persistent files belong below `<data-dir>/<engine-name>/`.

Discovery starts the executable, reads and validates `EngineHello`, registers
the complete schema atomically, and then closes the worker without sending
`HostHello`. Normal activation repeats `EngineHello`, receives `HostHello`,
and only then asks the engine to initialize its own implementation state.
Workers must therefore keep pre-hello work cheap and must tolerate the host
closing the channel after a discovery probe.

The Host allows 5 seconds for `EngineHello` and 60 seconds for the first
`init` request, which includes heavyweight startup after `HostHello`.

## Payload Encoding

| Item | Value |
|------|-------|
| Record separator | Newline |
| Field separator | Tab |
| Text fields | Lowercase hexadecimal UTF-8 bytes |
| End marker | `END` (optional; ignored by the host) |

## Requests

| Operation | Arguments | Response records |
|-----------|-----------|------------------|
| `init` | None | `OK` or `ERR` |
| `deactivate` | None | `OK` or `ERR` |
| `focus-in` | Context id | `OK`, optional `ACTIVE_MODE` |
| `focus-out` | Context id | `OK` or `ERR` |
| `reset` | Context id | `OK`, optional `ACTIVE_MODE` |
| `reload-config` | None | `OK` or `ERR` |
| `availability` | None | `AVAILABILITY`, optional `ACTIVE_MODE` |
| `process-key` | Context id and key fields | `RESULT`, composition records, optional `ACTIVE_MODE` |
| `process-audio` | Hexadecimal little-endian `f32` samples | Optional `TEXT` |
| `list-modes` | None | Zero or more `MODE` records |
| `get-active-mode` | Context id | Optional `ACTIVE_MODE` |
| `set-active-mode` | Context id and hexadecimal mode id | `OK` or `ERR`, optional `ACTIVE_MODE` |
| `commit-candidate` | Context id and candidate index | `OK`, `ERR`, or composition records |
| `list-commands` | None | Zero or more `COMMAND` records |
| `invoke-command` | Hexadecimal command id | `OK` or `ERR` |
| `shutdown` | None | Worker exits without a response |

## Response Records

| Record | Fields |
|--------|--------|
| `OK` | None |
| `ERR` | Error code or human-readable message; command errors use `NOT_FOUND` or `NOT_SUPPORTED` |
| `RESULT` | `NOT_HANDLED`, `HANDLED`, `COMPOSING`, `COMMITTED`, or `PASS_THROUGH` |
| `AVAILABILITY` | `UNINITIALIZED`, `PREPARING`, `READY`, or `FAILED` |
| `TEXT` | Hexadecimal UTF-8 text |
| `MODE` | Mode metadata fields |
| `ACTIVE_MODE` | Mode metadata fields |
| `COMMAND` | Hexadecimal id and hexadecimal label |
| `COMPOSITION` | Cursor, page, selection, segment, and candidate fields |
| `COMMIT` | Hexadecimal UTF-8 text |
| `CLEAR` | None |
