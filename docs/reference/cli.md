# Command-Line Interface Reference

## Commands

| Command | Purpose |
|---------|---------|
| `typio` | Start the Wayland input method daemon. |
| `typioctl` | Inspect or control the running daemon over TIP. |
| `typio-settings` | Open the graphical settings application. |

## Options

| Option | Argument | Effect |
|--------|----------|--------|
| `-c`, `--config` | `DIR` | Set the configuration directory. |
| `-d`, `--data` | `DIR` | Set the data directory. |
| `-E`, `--engine-dir` | `DIR` | Set the engine manifest directory override. |
| `-v`, `--verbose` | None | Enable debug logging. |
| `-vv` | None | Enable trace logging, including high-volume key-routing traces. |
| `-h`, `--help` | None | Print command-line help and exit. |
| `--version` | None | Print version information and exit. |

`typio-settings` accepts `-v`/`--verbose` to log raw Iris platform input and
`-h`/`--help` to print usage. Its schema-backed pages require a running daemon;
the Appearance page edits `platform.toml` directly and remains available while
the daemon is stopped.

## `typioctl` Subcommands

`typioctl` is the command-line client for a running daemon. It speaks TIP v3
over the UDS socket and reports an error when the daemon is absent. Every
command has the shape `<resource> <verb> [target] [args]`; the engine name or
language tag is always explicit, never implied by the active state (ADR-0004,
held in `docs/adr/archive/cli-control/`).

`-o, --output {plain|json}`, `-h, --help`, and `-V, --version` are global flags,
accepted at any level of the command tree. In plain mode a `*` marks the active
engine or language. `--output json` prints the raw result payload of the
underlying RPC without modification. `config show` prints the daemon config text
(TOML) in either mode.

### `engine`

| Command | RPC | Notes |
|---|---|---|
| `typioctl engine list` | `engine.list` | `*` marks the active engine. |
| `typioctl engine show <name>` | `engine.describe` | Properties, commands, and current values. |
| `typioctl engine use <name>` | `keyboard.use` or `voice.use` | Resolves the kind from `engine.list`, then dispatches to the matching modality verb. |
| `typioctl engine next [--kind keyboard\|voice]` | `keyboard.next` or `voice.next` | Defaults to the keyboard slot. |
| `typioctl engine props <name>` | `engine.describe` | Prints the `properties` slice. |
| `typioctl engine actions <name>` | `engine.describe` | Prints the `commands` slice. |
| `typioctl engine get <name> <key>` | `config.get` | Reads the key `engines.<name>.<key>`. |
| `typioctl engine set <name> <key> <value>` | `config.set` | Writes the key `engines.<name>.<key>`; the daemon delivers `on_config_change` to the engine. |
| `typioctl engine do <name> <command>` | `engine.invoke` | Runs a registered engine command. |
| `typioctl engine setup [name]` | `engine.invoke` | Invokes the engine's `setup` command (`{ name, command: "setup" }`). Without `name`, lists every engine exposing a `setup` command by walking `engine.list` and `engine.describe`. |
| `typioctl engine load <path>` | `engine.load` | Loads one engine manifest from an absolute `.toml` path. |
| `typioctl engine unload <name>` | `engine.unload` | Deactivates the engine first when it is active. |
| `typioctl engine reload <name> [--path <p>]` | `engine.reload` | With `--path`, loads from that path; otherwise rescans `engine_dirs`. |

### `language`

The language (a BCP 47 tag) is the user-facing switch unit: activating a
language retargets the keyboard and voice engine slots together. Requires daemon
`protocolVersion` ≥ 3.

| Command | RPC | Notes |
|---|---|---|
| `typioctl language list` | `language.list` | `*` marks the active language. |
| `typioctl language use <tag>` | `language.use` | Tag is BCP 47, e.g. `zh-Hans`, `ar-MA`. |
| `typioctl language next` | `language.next` | Prints the new active tag. |
| `typioctl language prev` | `language.prev` | Same, in the opposite direction. |

### `config`

| Command | RPC | Notes |
|---|---|---|
| `typioctl config get <key>` | `config.get` | Returns value, type, and source. |
| `typioctl config set <key> <value>` | `config.set` | The value is always a string; the daemon coerces it by schema type. |
| `typioctl config unset <key>` | `config.unset` | Reverts the key to its schema default. |
| `typioctl config list [--prefix <p>]` | `config.list` | Lists schema entries, optionally filtered by prefix. |
| `typioctl config show` | `config.show` | Raw daemon config text (TOML). |
| `typioctl config edit` | `config.show`, then `$EDITOR` | Read-only preview in TIP v3: `config.set` takes typed keys, not whole-file text, so an editor round trip reports that no write was applied. |
| `typioctl config reload` | `config.reload` | Re-reads the config from disk. |

### `daemon`

| Command | RPC |
|---|---|
| `typioctl daemon status` | `daemon.status` |
| `typioctl daemon stop` | `daemon.stop` |
| `typioctl daemon version` | `daemon.version` |

## Log Levels

The verbosity flag sets the **global floor** for the daemon's `tracing`
output. Every diagnostic flows through `tracing` — there is no separate
`stdout`/`stderr` print channel.

| Invocation | Minimum log level |
|------------|-------------------|
| `typio` | `info` |
| `typio -v` | `debug` |
| `typio --verbose` | `debug` |
| `typio -vv` | `trace` |
| `typio --verbose --verbose` | `trace` |

At `info`, output is the startup self-check (registered engines, connected
surfaces), lifecycle milestones, warnings, and errors. `debug` adds
per-event subsystem diagnostics (indicator, panel, voice, tray, switch
chords). `trace` adds per-keystroke routing and per-frame timing.

Output is colorized only when stderr is an interactive terminal; under the
systemd journal or a redirected file it is plain text.

## Log Filtering with `RUST_LOG`

`RUST_LOG` refines individual subsystems on top of the global floor without
raising it everywhere. Each event carries a `typio.<subsystem>[.<area>]`
target:

| Target | Subsystem |
|--------|-----------|
| `typio.startup` | Daemon init: engine registration, surface/IPC setup. |
| `typio.lifecycle` | Run/reload/shutdown/restart transitions. |
| `typio.indicator` | On-screen language/mode indicator driving. |
| `typio.voice` | Voice push-to-talk and transcription. |
| `typio.tray` | Tray menu actions. |
| `typio.config` | Config-file watcher. |
| `typio.panel.*` | Candidate panel scheduling, host, timing, and probe events. |
| `typio.wayland.*` | Wayland I/O, grab, keymap, frontend. |
| `typio.engine.*` | Engine key processing, composition, selection. |
| `typio.input.*` | Input queue. |

Examples:

```bash
# Trace only the indicator; everything else stays at info.
RUST_LOG=typio.indicator=trace typio

# Quiet the panel while debugging voice.
RUST_LOG=typio.voice=debug,typio.panel=warn typio -v
```

## Runtime Signals

| Signal | Effect |
|--------|--------|
| `SIGUSR1` | Raise the running daemon log level by one step: `info` to `debug`, `debug` to `trace`. |
| `SIGUSR2` | Reset the running daemon log level to the startup level. |

`SIGUSR1`/`SIGUSR2` adjust the global floor on a live daemon without a
restart — the runtime equivalent of `-v`/`-vv`. Per-target `RUST_LOG`
directives set at startup are preserved across the change.
