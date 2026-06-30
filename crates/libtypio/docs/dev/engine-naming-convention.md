# Engine Naming Convention

This document defines mandatory naming rules for Typio engines.

## Scope

| Item | Rule |
|------|------|
| Repository | `typio-engine-<name>` |
| Manifest | `typio-engine-<name>.toml` |
| Runtime name | `<name>` |
| Engine executable | `typio-engine-<name>` |
| Installed engine executable | `<libexecdir>/typio/engines/typio-engine-<name>` |
| Installed manifest | `<datadir>/typio/engines/typio-engine-<name>.toml` |

## Runtime Name Rules

| Rule | Value |
|------|-------|
| Character set | Lowercase ASCII letters, digits, hyphens |
| Word style | Kebab-case |
| Upstream wrapper | Match the upstream project name |
| Type suffixes | Do not include `keyboard`, `voice`, or `handwriting` |
| Generic suffixes | Avoid `typio`, `plugin`, `engine` |

## Examples

| Engine | Type | Repository | Manifest | Runtime name |
|--------|------|------------|----------|--------------|
| Rime | Keyboard | `typio-engine-rime` | `typio-engine-rime.toml` | `rime` |
| Mozc | Keyboard | `typio-engine-mozc` | `typio-engine-mozc.toml` | `mozc` |
| Sherpa-ONNX | Voice | `typio-engine-sherpa` | `typio-engine-sherpa.toml` | `sherpa-onnx` |

## Host Responsibilities

| Responsibility | Owner |
|----------------|-------|
| Directory resolution | Host |
| Manifest filename filtering | Host |
| Manifest parsing | Host |
| Capability negotiation | Host |
| Engine process registration | `typio_registry_register_engine_process` |

## See Also

- [ADR-0017](../adr/0017-typio-engine-protocol.md)
- [Project Layout](project-layout.md)
