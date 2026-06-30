# Installation Layout Reference

This page describes every file and directory installed when building
`libtypio` from source.

Paths are shown relative to the installation prefix (default `/usr/local`).
Override by copying to a different prefix.

## Libraries

| Path | Type | Description |
|------|------|-------------|
| `lib/libtypio.so` | Shared library | Core library (C ABI / Rust implementation). Loaded by hosts and C/C++ engine executables. |
| `lib/libtypio.a` | Static library | Static archive for linking hosts or engine executables directly. |

## Headers

| Path | Description |
|------|-------------|
| `include/typio/*.h` | Public C API headers. Required for building C/C++ engine executables and hosts. |

## Engine Packages (installed separately)

Engines are built in their own repositories:

| Path | Description |
|------|-------------|
| `libexec/typio/engines/typio-engine-*` | Private engine executables started by the host. |
| `share/typio/engines/typio-engine-*.toml` | Engine metadata and command lines discovered by the host. |

## Runtime Directories (not installed)

These directories are created at runtime:

| Path | Description |
|------|-------------|
| `~/.config/typio/` | User configuration directory. Contains `core.toml`, frontend config such as `wayland.toml`, and optional engine config files. |
| `~/.local/share/typio/` | User data directory. Contains engine-side artifacts such as Rime deploy output and engine caches. Core logging is host-driven (see [Logging API Reference](host-abi/log.md)) and writes no files itself. |
| `~/.local/state/typio/` | User state directory. Contains runtime state such as last-used keyboard/voice engine pairs and per-application engine/mode state. |
| `/run/user/$UID/typio/` | Runtime directory. Contains the UDS socket for CLI-daemon communication. |

## Quick Verification

After building, verify the artifacts:

```bash
# Build
cargo build --release

# Check library artifact
ls -la target/release/libtypio.so target/release/libtypio.a

# Check headers
ls include/typio/

# Staging install (no root needed)
mkdir -p /tmp/typio-staging/usr/local/lib /tmp/typio-staging/usr/local/include
cp target/release/libtypio.so target/release/libtypio.a /tmp/typio-staging/usr/local/lib/
cp -r include/typio /tmp/typio-staging/usr/local/include/
find /tmp/typio-staging -type f -o -type l | sort
rm -rf /tmp/typio-staging
```
