# Engine Reference

Optional input engines. The framework itself ships no engine and supports
the zero-engine state.

## Engine Types

| Type | Slot | Purpose |
|------|------|---------|
| `keyboard` | Primary | Key processing, preedit, candidates, commit |
| `voice` | Secondary | Speech-to-text audio inference |

Keyboard and voice selections are independent. Switching one never evicts the other.

---

## Keyboard Engines

### `compose`

Zero-dependency Latin keyboard engine. Commits printable Unicode text directly
and provides a Shift+Alt compose picker for accented characters.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `engines.compose.printable_key_mode` | string | `"commit"` | `"commit"` commits directly; `"forward"` forwards through virtual keyboard |
| `engines.compose.compose` | bool | `false` | Enable dead-key compose sequences (e.g. `'` + `a` → `á`) |

Capabilities: `TYPIO_CAP_NONE`

Modes: `native` (implicit; no mode surface).

Candidate selection is partially host-managed. The compose engine produces
candidates; `typio` handles navigation and commit policy.

Compose sequences (when `compose = true`):

| Sequence | Result |
|----------|--------|
| `'` + vowel | acute accent (á, é, í, ó, ú) |
| `` ` `` + vowel | grave accent (à, è, ì, ò, ù) |
| `^` + vowel | circumflex (â, ê, î, ô, û) |
| `"` + vowel | diaeresis (ä, ë, ï, ö, ü) |
| `~` + `n` | tilde (ñ) |

Escape cancels an active composition and clears preedit.

---

### `rime`

Chinese input powered by [librime](https://github.com/rime/librime). Ships as the out-of-tree engine `typio-engine-rime`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `engines.rime.shared_data_dir` | string | `/usr/share/rime-data` | System Rime data directory |
| `engines.rime.user_data_dir` | string | `~/.local/share/typio/rime` | Per-user Rime data directory |
| `engines.rime.full_check` | bool | implied by deploy | Controls whether deployment runs a full schema check |

Capabilities:
`TYPIO_CAP_PREEDIT | TYPIO_CAP_CANDIDATES | TYPIO_CAP_PREDICTION | TYPIO_CAP_LEARNING`

Modes:

| `id` | `label` | `display_label` | `profile_id` (schema) |
|------|---------|-----------------|-----------------------|
| `native` | Native | 中 | current schema (e.g. `luna_pinyin`) |
| `ascii` | ASCII | A | — |

Mode is derived from the Rime `ascii_mode` option. Shift toggles it when the schema's `ascii_composer` is configured to use `Shift` as a switch key and the Wayland frontend consumes the handled modifier (see [Modifier Key Consumption](../explanation/modifier-key-consumption.md)).

Candidate selection: engine-managed by default (librime handles navigation internally). Engines may opt in to host-managed selection via `host_managed_selection` flags in future versions.

Rime configuration (`<user_data_dir>/default.custom.yaml`):

| Patch key | Valid values | Default | Description |
|-----------|-------------|---------|-------------|
| `ascii_composer/switch_key/Shift_L` | `commit_code`, `commit_text`, `inline_ascii`, `clear`, `noop`, `set_ascii_mode`, `unset_ascii_mode` | `inline_ascii` | Action when Left Shift is pressed |
| `ascii_composer/switch_key/Shift_R` | same as above | `commit_text` | Action when Right Shift is pressed |

**Important:** `toggle` is **not** a valid value for `ascii_composer/switch_key`. It is a `key_binder` action, not an `ascii_composer` switch key value. Using `toggle` here silently disables Shift switching.

Schema deployment requirement: a schema must be listed in `patch.schema_list` for `rime_deployer` to compile its binary tables. Adding `.schema.yaml` and `.dict.yaml` files is not enough without the `schema_list` entry.

Session behavior:
- One Rime session per `TypioInputContext`, stored as a context property.
- Sessions survive focus churn so runtime options (e.g. `ascii_mode`) are preserved.
- Deployment invalidates all existing sessions; they are recreated lazily on next focus.
- If Rime is still deploying, key presses show a temporary preedit message (`… Rime 正在部署`) instead of blocking.

Config reload rules:
- Changing `shared_data_dir` or `user_data_dir` requires restarting Typio.
- Explicit deploy (via D-Bus or control panel) invalidates generated YAML and triggers a full rebuild.

Learning & persistence:
- User-dictionary learning is automatic. librime records each commit into a per-schema LevelDB at `<user_data_dir>/<schema>.userdb/` and persists it across restarts (no action needed from Typio).
- Disable it per schema by patching `translator/enable_user_dict: false` in `default.custom.yaml`.
- Cross-device dictionary sync (librime's `sync_user_data`) is **not implemented and not planned** — it is out of scope for a basic input method. Back up `user_data_dir` to carry learning between machines. See the `typio-engine-rime` repository's `docs/explanation/librime-integration.md` (§8) for details.

---

### `mozc`

Japanese input via [Mozc](https://github.com/google/mozc) server IPC. Ships as the out-of-tree engine `typio-engine-mozc`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `engines.mozc.server_path` | string | `/usr/lib/mozc/mozc_server` | Path to `mozc_server` executable |

Capabilities: `TYPIO_CAP_PREEDIT | TYPIO_CAP_CANDIDATES`

Modes:

| Mozc `CompositionMode` | `id` | `display_label` | Notes |
|------------------------|------|-----------------|-------|
| `DIRECT` | `direct` | A | Direct input |
| `HIRAGANA` | `hiragana` | あ | Default Japanese mode |
| `FULL_KATAKANA` | `full_katakana` | カ | Full-width katakana |
| `HALF_KATAKANA` | `half_katakana` | ｶ | Half-width katakana |
| `FULL_ASCII` | `full_ascii` | Ａ | Full-width ASCII |
| `HALF_ASCII` | `half_ascii` | A | Half-width ASCII |

IPC behavior:
- Protocol: `[size:4 LE][protobuf]` over Unix domain socket.
- Socket path resolution order:
  1. Abstract socket matching `@tmp/.mozc.*.session` (read from `/proc/net/unix`).
  2. `$XDG_CONFIG_HOME/mozc/session.ipc`
  3. `~/.mozc/session.ipc`
- If the server is not running, Typio attempts to launch it once per session.
- Each RPC uses a fresh connection with a 300 ms timeout.
- Session creation failure triggers a 3-second retry backoff.

Focus handling:
- `focus_in` re-activates the session if it was left in an ASCII mode.
- `focus_out` submits the current composition (`REVERT`) and clears UI state.
- `reset` sends `RESET_CONTEXT` and clears Typio-side state.

---

## Voice Engines

Voice engines implement `TYPIO_ENGINE_TYPE_VOICE`. They do not handle key events; instead they expose a `TypioVoiceEngineOps` vtable through `engine->voice` that the voice service calls during inference.

Voice engines are external engine processes, not built into `libtypio.so`.
Their manifests declare `type = "voice"`, which selects libtypio's voice
registry and audio-processing strategy.

### `whisper`

Speech-to-text via [whisper.cpp](https://github.com/ggerganov/whisper.cpp). Ships as the out-of-tree engine `typio-engine-whisper`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `engines.whisper.language` | string | `"auto"` | BCP-47 language code or `"auto"` |
| `engines.whisper.model` | string | `"base"` | Model name; loads `~/.local/share/typio/whisper/ggml-<name>.bin` |

Capabilities: `TYPIO_CAP_VOICE_INPUT`

Model file layout:

```text
~/.local/share/typio/
└── whisper/
    └── ggml-<model>.bin
```

Supported model names depend on the whisper.cpp build (commonly `tiny`, `base`, `small`, `medium`, `large`).

Config reload behavior:
- Reload is **non-blocking**: a background thread loads the new model and hot-swaps it.
- The inference thread holds a reference-counted snapshot of the old backend, so in-flight recognition is never interrupted.
- If reload is requested while recording or processing, it is deferred until the job finishes.

Build and install:
```bash
cd typio-engine-whisper
cargo build --release
```

---

### `sherpa-onnx`

Speech-to-text via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx). Ships as the out-of-tree engine `typio-engine-sherpa`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `engines.sherpa-onnx.language` | string | `"auto"` | Language hint (backend-specific interpretation) |
| `engines.sherpa-onnx.model` | string | first found | Subdirectory name under `~/.local/share/typio/sherpa-onnx/` |

Capabilities: `TYPIO_CAP_VOICE_INPUT`

Model directory layout:

```text
~/.local/share/typio/
└── sherpa-onnx/
    └── <model-name>/
        ├── tokens.txt
        ├── model.onnx | model.int8.onnx        # SenseVoice / Paraformer
        ├── encoder.onnx + decoder.onnx          # Whisper / Transducer
        └── joiner.onnx                          # Transducer only
```

Auto-detection:
- If `model` is omitted, the engine scans `sherpa-onnx/` subdirectories and picks the first one with a recognizable file layout.
- Model type is detected from file presence, not from config:

| Detected type | Required files |
|---------------|----------------|
| SenseVoice | `tokens.txt` + `model.int8.onnx` or `model.onnx`; directory name contains `sensevoice` or `sense-voice` |
| Paraformer | `tokens.txt` + `model.int8.onnx` or `model.onnx`; no `sensevoice` in directory name |
| Transducer | `tokens.txt` + `encoder.onnx` + `decoder.onnx` + `joiner.onnx` |
| Whisper (ONNX) | `tokens.txt` + `encoder.onnx` + `decoder.onnx` (no joiner) |

Config reload behavior:
- Same non-blocking hot-swap design as Whisper.

Build and install:
```bash
cd typio-engine-sherpa
cargo build --release
```

---

## Engine Capability Flags

| Flag | Value | Meaning |
|------|-------|---------|
| `TYPIO_CAP_NONE` | `0` | No special capabilities |
| `TYPIO_CAP_PREEDIT` | `1 << 0` | Engine produces preedit text |
| `TYPIO_CAP_CANDIDATES` | `1 << 1` | Engine produces candidate lists |
| `TYPIO_CAP_PREDICTION` | `1 << 2` | Engine supports prediction |
| `TYPIO_CAP_LEARNING` | `1 << 3` | Engine supports user dictionary learning |
| `TYPIO_CAP_VOICE_INPUT` | `1 << 4` | Engine is a voice/STT backend |

---

## Engine Mode System

Keyboard engines declare modes via `TypioKeyboardEngineOps`:

```c
typedef struct TypioKeyboardEngineMode {
    const char *id;             /* Stable identifier: "native", "ascii", "browse" */
    const char *label;          /* Human-readable name: "Native", "ASCII" */
    const char *display_label;  /* Short badge: "中", "A", "Browse" */
    const char *icon_name;      /* Freedesktop icon name */

    /* Profile — engine-defined active profile (e.g. Rime schema) */
    const char *profile_id;     /* "luna_pinyin", "wubi86" */
    const char *profile_label;  /* "朙月拼音", "五笔86" */
    const char *description;    /* Optional detailed description */

    TypioStatusSalience salience;  /* QUIET or NOTABLE */
} TypioKeyboardEngineMode;
```

Engines implement three optional mode ops:

- `list_modes` — return all supported modes
- `get_active_mode` — return the current mode
- `set_active_mode` — accept or reject a mode switch request (NULL id = cycle to next)

Engines notify the host of mode changes by calling `typio_instance_notify_keyboard_mode()`. The framework compares `id` for change detection; all other fields are presentation-only.

---

## Engine ABI

External engines are shared objects that export:

**Keyboard engine:**
```c
const TypioEngineInfo *typio_engine_get_info(void);
TypioKeyboardEngine *typio_keyboard_engine_create(void);
```

**Voice engine:**
```c
const TypioEngineInfo *typio_engine_get_info(void);
TypioVoiceEngine *typio_voice_engine_create(void);
```

See [Engine Operations](engine/ops.md) for the full `TypioEngineBaseOps` / `TypioKeyboardEngineOps` / `TypioVoiceEngineOps` vtables and [How to Create a Custom Keyboard Engine](../how-to/create-custom-keyboard-engine.md) or [How to Create a Custom Voice Engine](../how-to/create-custom-voice-engine.md) for a minimal example.

Install path for engines:

| Build type | Path |
|------------|------|
| System install | `${prefix}/lib/typio/engines/*.so` |
| Custom prefix | `${TYPIO_INSTALL_ENGINEDIR}/*.so` |
