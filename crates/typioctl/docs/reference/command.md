# CLI Reference

`typioctl` is the command-line client (ADR-0004). It speaks the daemon's
TIP v1 protocol over the UDS socket at `$XDG_RUNTIME_DIR/typio/daemon.sock`.

## Global flags

| Flag | Default | Meaning |
|---|---|---|
| `-o, --output {plain|json}` | `plain` | Output format. `json` prints the raw result payload from the underlying RPC. |
| `-h, --help` | — | Print contextual help. |
| `-V, --version` | — | Print the CLI version. |

## `engine`

| Command | RPC | Notes |
|---|---|---|
| `engine list` | `engine.list` | `*` marks the active one (plain mode). |
| `engine show <name>` | `engine.describe` | Properties + commands + values. |
| `engine use <name>` | `engine.use` | Kind (keyboard / voice) inferred from the engine info. |
| `engine next [--kind voice]` | `engine.next` | Default kind is keyboard. |
| `engine props <name>` | `engine.describe` | Filters to the `properties` slice. |
| `engine actions <name>` | `engine.describe` | Filters to the `commands` slice. |
| `engine get <name> <key>` | `config.get` | Underlying key is `engines.<name>.<key>`. |
| `engine set <name> <key> <value>` | `config.set` | Triggers `on_config_change` on the engine. |
| `engine do <name> <command>` | `engine.invoke` | Runs a registered engine command. |
| `engine setup [name]` | `engine.setup` | One-click setup for an engine. No name lists available setups. |
| `engine load <path>` | `engine.load` | Load an engine from a specific manifest path. |
| `engine unload <name>` | `engine.unload` | Unload an engine by name. |
| `engine reload <name> [--path <p>]` | `engine.reload` | Reload an engine. With `--path`, loads from that path; otherwise rescans engine_dirs. |

## `language`

The language (a BCP-47 tag) is the user-facing switch unit: activating a
language retargets the keyboard and voice engine slots together
(typio ADR-0031). Requires daemon protocolVersion ≥ 3.

| Command | RPC | Notes |
|---|---|---|
| `language list` | `language.list` | `*` marks the active one (plain mode). |
| `language use <tag>` | `language.use` | Tag is BCP-47, e.g. `zh-Hans`, `ar-MA`. |
| `language next` | `language.next` | Prints the new active tag. Errors when no languages are enabled or declared. |
| `language prev` | `language.prev` | Same, backwards. |

## `config`

| Command | RPC | Notes |
|---|---|---|
| `config get <key>` | `config.get` | Returns value, type, and source. |
| `config set <key> <value>` | `config.set` | Value is always a string; daemon coerces by schema type. |
| `config unset <key>` | `config.unset` | Reverts to schema default. |
| `config list [--prefix p]` | `config.list` | Lists schema entries (optionally filtered). |
| `config show` | `config.show` | Raw daemon config text (TOML). |
| `config reload` | `config.reload` | Re-reads from disk. |
| `config edit` | `config.show` then editor | Read-only preview in TIP v1; bulk writes not yet supported. |

## `daemon`

| Command | RPC |
|---|---|
| `daemon status` | `daemon.status` |
| `daemon stop` | `daemon.stop` |
| `daemon version` | `daemon.version` |

## Examples

```sh
# Switch language (retargets keyboard + voice engines together)
typioctl language list
typioctl language use zh-Hans

# Switch to Rime and select a schema
typioctl engine use rime
typioctl engine set rime schema luna_pinyin

# Run a Rime-side maintenance step after editing default.custom.yaml
typioctl engine do rime deploy

# Reload rime engine during development (from build directory)
typioctl engine reload rime --path ./build/typio-engine-rime.toml

# Unload and reload rime from the standard engine directories
typioctl engine unload rime
typioctl engine reload rime

# Machine-readable status snapshot
typioctl -o json daemon status
```
