# Engine Protocol Reference

## Identity

| Item | Value |
|------|-------|
| Name | Typio Engine Protocol |
| Manifest value | `typio-engine-protocol` |
| Version | `1.0` |
| Magic | `TYEP` |
| Header | `typio/abi/engine_protocol.h` |
| Rust module | `core::engine::backend::engine_protocol` |

## Transport

| Item | Value |
|------|-------|
| Process model | Host starts one engine executable on demand |
| Protocol fd | `3` |
| Environment | `TYPIO_ENGINE_PROTOCOL=1.0`, `TYPIO_ENGINE_FD=3` |
| Standard input | Closed to engine protocol traffic |
| Standard output | Logs only |
| Standard error | Logs only |
| Host registration | `typio_registry_register_engine_process` |

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
| Maximum payload | 1 MiB |
| Major version mismatch | Transport error |
| Response request id | Must echo request id |

## Hello Payload

```text
protocol	1.0
engine	<name>
type	<keyboard|voice>
```

## Payload Schema

| Phase | Value |
|-------|-------|
| Current payload | Existing request and response line semantics inside framed payloads |
| Future payload | Typed schema can replace line payloads without returning to stdio transport |
