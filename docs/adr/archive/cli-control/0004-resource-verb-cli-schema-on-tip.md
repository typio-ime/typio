# ADR-0004: Resource+verb CLI schema on TIP v1

- **Status**: Accepted
- **Date**: 2026-05-29
- **Deciders**: Project maintainers
- **Pointer**: the IPC mapping below records the TIP v1 surface as decided on 2026-05-29. It predates modality-explicit activation (`keyboard.*` / `voice.*`) and the `engine setup` command; the current `typioctl` mapping lives in the [command-line interface reference](../../../reference/cli.md).

## Context

The pre-v2 CLI grew organically. `typioctl engine` accepted `list`, `next`, or *any engine name* as its single positional arg — a collision with an engine named `next`. `typioctl rime schema X` *set* the schema; `typioctl rime schema` *got* it; `typioctl rime X` was an undocumented fallback that also set it. `typioctl config set "<entire TOML blob>"` replaced the whole daemon config file. Top-level `status`, `stop`, `version` sat alongside grouped commands.

The new daemon IPC (typio-wayland ADR-0008) lays down a resource-oriented JSON-RPC vocabulary (`config.*`, `engine.*`, `daemon.*`, `events.subscribe`). libtypio ADR-0008 unified per-engine properties into the config tree so the CLI no longer needs an engine-specific `rime` namespace. This ADR records the CLI shape that uses those APIs cleanly.

## Decision

Adopt a kubectl/docker-style **resource + verb** schema. Engine name is always positional after the verb (no implicit "active"); the namespace is uniform.

```text
typioctl engine list                     # all engines (* marks active)
typioctl engine show <name>              # properties + commands + values
typioctl engine use <name>               # set active
typioctl engine next [--kind voice]      # cycle within a kind (default keyboard)
typioctl engine props <name>             # introspect schema
typioctl engine actions <name>           # list commands
typioctl engine get <name> <key>         # e.g. engine get rime schema
typioctl engine set <name> <key> <val>   # e.g. engine set rime schema luna_pinyin
typioctl engine do  <name> <command>     # e.g. engine do rime deploy

typioctl config get   <key>
typioctl config set   <key> <value>
typioctl config unset <key>
typioctl config list  [--prefix <p>]
typioctl config show                     # dump (TOML)
typioctl config edit                     # $EDITOR roundtrip
typioctl config reload

typioctl daemon status
typioctl daemon stop
typioctl daemon version

# global flag
  -o, --output {plain|json}              default: plain
```

### Removed surface

- `typioctl rime` — every form. `rime deploy` becomes `engine do rime deploy`; `rime schema X` becomes `engine set rime schema X` (which calls `config.set engines.rime.schema X` underneath).
- `typioctl engine NAME` and `typioctl engine next/list` colliding at the same positional slot. Replaced by explicit verbs.
- `typioctl config set "<entire text>"` — replaced by typed key/value writes; `config edit` covers the whole-file editing use case.
- Top-level `status`, `stop`, `version` — moved under `daemon`.
- Verbose help-on-error catch-all — clap's built-in `--help` output is the contract.

### IPC mapping

| CLI | Method |
|---|---|
| `engine list` | `engine.list` |
| `engine show <name>` | `engine.describe { name }` |
| `engine use <name>` | `engine.use { name }` |
| `engine next` | `engine.next { kind: "keyboard" }` |
| `engine get <name> <key>` | `config.get { key: "engines.<name>.<key>" }` |
| `engine set <name> <key> <value>` | `config.set { key: "engines.<name>.<key>", value }` |
| `engine do <name> <command>` | `engine.invoke { name, command }` |
| `engine props <name>` | reuses `engine.describe`, prints `properties` only |
| `engine actions <name>` | reuses `engine.describe`, prints `commands` only |
| `config get <key>` | `config.get { key }` |
| `config set <key> <value>` | `config.set { key, value }` |
| `config unset <key>` | `config.unset { key }` |
| `config list` | `config.list { prefix }` |
| `config show` | `config.show {}` |
| `config edit` | `config.show {}` → `$EDITOR` → `config.set` per changed key |
| `config reload` | `config.reload {}` |
| `daemon status` | `daemon.status {}` |
| `daemon stop` | `daemon.stop {}` |
| `daemon version` | `daemon.version {}` |

### Output

- `--output plain` (default): human-readable. Tables use ASCII alignment; markers like `*` flag the active engine; `engine show` renders properties as `key = value (type)` blocks.
- `--output json`: the raw `result` payload from the underlying RPC, unmodified. Scripts and downstream tools key off field names from the IPC reference doc.

`config show` always prints the raw daemon config text (TOML) regardless of `--output`; it has no structured representation.

## Alternatives considered

- **Verb-first (git-style): `typioctl list engines`, `typioctl use engine rime`.** Rejected: terser for one-offs but verbs proliferate over time. Resource+verb scales as new resources are added without crowding a flat verb namespace, and aligns with the IPC method-name shape.

- **Implicit-active engine target: `engine get schema` operates on the current active engine.** Rejected (per earlier design discussion): readable for interactive use but ambiguous in scripts when the active engine changes between commands. Explicit `<name>` removes the foot-gun.

- **Keep `rime` as a deprecated alias for one release.** Rejected per project mandate (greenfield rewrite, no backward compatibility shims). In-tree consumers of the CLI port in the same wave; documented shell scripts get a one-time `sed` migration.

- **Keep top-level `status`, `stop`, `version` shortcuts.** Rejected for namespace consistency. The few extra characters (`typioctl daemon status` vs `typioctl status`) are not worth the asymmetry with the rest of the surface.

- **Add `engine reload` distinct from `daemon stop` and `config reload`.** Rejected: the underlying daemon does not expose an engine-level reload, and the existing `config.reload` already triggers per-engine reload through `on_config_change`.

## Consequences

- Positive: the CLI surface is fully predictable. Each command has the shape `<resource> <verb> [target] [args]`; help discovery is one `--help` deep at any level.
- Positive: machine-readable mode (`-o json`) makes shell scripting straightforward and removes the need for fragile output parsing.
- Positive: every engine-specific command (rime's `deploy`, future engines') is reachable through the same `engine do <name> <command>` path — adding a new engine adds zero CLI surface.
- Positive: removing the `rime` namespace removes ~70 lines of `cmd_rime*` plumbing in `src/commands.rs` and the special-case dispatch in `src/main.rs`.
- Trade-off: command lines for common ops grow slightly. `typioctl rime deploy` (16 chars after the binary name) becomes `typioctl engine do rime deploy` (25). The tradeoff is uniformity and discoverability.
- Negative (accepted): users with shell aliases or scripts targeting the old surface must migrate in one step. The repository README will lead with the new vocabulary; the previous form is documented as removed.

## Related

- [typio ADR-0008: TIP v1 — IPC Protocol](../../0008-ipc-protocol-resource-namespaces-uds-only.md) — the underlying RPC vocabulary the CLI maps to.
- libtypio ADR-0008, *engine properties unified into the config schema* — why the `rime` namespace is no longer needed. The `libtypio` crate and its ADR tree were retired, so this record has no link target.
- [ADR-0003: CLI binary naming](0003-cli-binary-naming.md) — establishes `typioctl` as the binary name this ADR's surface is exposed through.
