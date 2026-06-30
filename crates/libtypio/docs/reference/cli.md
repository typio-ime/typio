# CLI Reference

The `typio` executable is the Linux host daemon. The separate `typioctl`
executable controls a running daemon over UDS.

## Executables

| Executable | Owner | Purpose |
|------------|-------|---------|
| `typio` | `typio` | Linux input-method host daemon |
| `typioctl` | `typioctl` | Engine, config, status, and lifecycle client |

| `typio` flag | Description |
|--------------|-------------|
| `-E`, `--engine-dir DIR` | Add an engine manifest directory |
| `-v`, `--verbose` | Enable debug logging |
| `--version` | Print version |
| `--help` | Print help |

The authoritative command references live in the `typio` and
`typioctl` repositories.
