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
