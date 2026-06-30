# Configuration Reference

Typio's configuration is split across files, one per process boundary:

| File | Owner | Scope |
|------|-------|-------|
| `$XDG_CONFIG_HOME/typio/core.toml` | libtypio | Keyboard policy, notifications, shortcuts, voice runtime, per-engine settings |
| `$XDG_CONFIG_HOME/typio/platform.toml` | `typio` | Popup theme, layout, fonts, color overrides |
| `$XDG_CONFIG_HOME/typio/engines/<name>.toml` (where applicable) | individual engine plugins | Engine-internal data not surfaced via the schema |

If `XDG_CONFIG_HOME` is unset, the directory falls back to `~/.config/typio`.
`$XDG_DATA_HOME/typio` (default `~/.local/share/typio`) holds user data.
`$XDG_STATE_HOME/typio` (default `~/.local/state/typio`) holds runtime state
such as the last-used keyboard and voice engine pairs.

This page documents `core.toml` only. For `wayland.toml` see the
`typio` repository's `docs/reference/configuration.md`.

## Runtime state

| File | Owner | Scope |
|------|-------|-------|
| `$XDG_STATE_HOME/typio/engine-state.toml` | libtypio | Last-used keyboard and voice engine pairs, and the active language (`[language] active`) |
| `$XDG_STATE_HOME/typio/identity-engine-state.toml` | `typio` | Per-application engine and mode state |

## Top-level keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|

## Language keys ([ADR-0018](../adr/0018-language-first-switching.md))

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `languages.enabled` | array of strings (or comma-separated string) | every engine-declared language, registration order | Ordered language cycle for `switch_language` (BCP-47 tags) |
| `languages.<tag>.keyboard` | string | first keyboard engine declaring `<tag>` | Keyboard engine for `<tag>`; `"none"` forces an empty slot (layout-only passthrough) |
| `languages.<tag>.voice` | string | first voice engine declaring `<tag>` | Voice engine for `<tag>`; `"none"` forces an empty slot |

## Shortcut keys

| Key | Default | Description |
|-----|---------|-------------|
| `shortcuts.switch_language` | `Ctrl+Shift` | Cycle the enabled language list |
| `shortcuts.switch_keyboard_engine` | *(unbound)* | Cycle keyboard engines within the registry |
| `shortcuts.exit` | `Ctrl+Shift+Escape` | Emergency daemon shutdown |
| `shortcuts.voice_ptt` | `Super+v` | Push-to-talk for voice input |

## Engine-owned sections

Every `[engines.<name>]` block is owned by the corresponding engine plugin,
not by libtypio. Each engine registers its keys, types, defaults, and UI
metadata via [`typio_config_schema_register*`](host-abi/schema.md) at plugin
load — the host enumerates them through the same `typio_config_schema_fields`
API it uses for the static base.

### Engine-scoped directories

Engines should call `typio_instance_get_engine_data_dir(instance, name)` from
their `init` callback to obtain a dedicated data directory
(`<data_dir>/<engine_name>/`, e.g. `~/.local/share/typio/rime/`). The path is
created automatically on first access and the pointer remains valid for the
lifetime of the instance. `typio_instance_get_engine_state_dir` provides the
same contract under `<state_dir>/<engine_name>/`.

This removes the need for engines to ask users to configure `user_data_dir` in
`core.toml`. Internal directory layout is entirely engine-defined.

Stock engines and the keys they currently register (authoritative list in
[Engine Reference](engines.md)):

| Section | Plugin | Keys |
|---------|--------|------|
| `[engines.compose]` | `typio-engine-compose` | `printable_key_mode`, `compose` |
| `[engines.rime]` | `typio-engine-rime` | `shared_data_dir`, `full_check` |
| `[engines.mozc]` | `typio-engine-mozc` | `server_path` |
| `[engines.whisper]` | `typio-engine-whisper` | `language`, `model` |
| `[engines.sherpa-onnx]` | `typio-engine-sherpa` | `language`, `model` |

Out-of-tree engines may add additional sections. Unknown `engines.<name>.*`
keys are preserved on read and round-trip on write so a config remains valid
when an engine plugin is temporarily absent.

## Path expansion

For Rime paths, `shared_data_dir` supports:

- `~` at the start of the path
- `$VAR`
- `${VAR}`

## Environment variable overrides

| Variable | Effect |
|----------|--------|
| `XDG_CONFIG_HOME` | Overrides config directory |
| `XDG_DATA_HOME` | Overrides data directory |

## See also

- [How to configure](../how-to/configure.md) — task-oriented walkthrough
- [Configuration System](../explanation/configuration-system.md) — design rationale
