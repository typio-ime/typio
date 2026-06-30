# Engine Icon Reference

## Valid Values

Engines set their icon through `TypioEngineInfo.icon`.

| Format | Example | When to use |
|--------|---------|-------------|
| **Freedesktop icon name** | `"input-keyboard"`, `"fcitx-rime"` | **Preferred.** The host resolves it through the current icon theme. |
| **Absolute file path** | `"/usr/share/typio/icons/my-engine.svg"` | **Deprecated.** Use freedesktop names instead. |

## Icon Distribution

Engines ship icons using the standard Freedesktop icon-theme layout. There are two ways to make them available to the host.

### System Installation (Standard)

For engines distributed through package managers or installed system-wide, icons should be installed to the standard XDG icon path:

```text
/usr/share/icons/hicolor/
└── scalable/
    └── apps/
        ├── typio-engine-myengine.svg
        └── typio-engine-myengine-symbolic.svg
```

> **Tip:** Ship icons as **SVG** only. A **`-symbolic.svg`** variant is strongly recommended. Symbolic icons are single-colour SVGs that GTK/Qt panels tint automatically to match the current theme, giving users consistent light/dark tray appearance without shipping multiple colour variants.

The host (and the user's desktop environment) resolves the engine's `icon` name through the normal icon theme lookup. This is the standard approach for distribution packaging.

### Bundled Icons (Self-contained)

For portable or user-local installations, engines may bundle icons next to
their manifest. The host scans the manifest directory for an `icons/`
subdirectory:

```text
<engine-install-dir>/
├── typio-engine-myengine.toml
└── icons/                    ← discovered by the host
    └── hicolor/
        └── scalable/
            └── apps/
                ├── typio-engine-myengine.svg
                └── typio-engine-myengine-symbolic.svg
```

The host scans each engine directory for an `icons/` subdirectory and exposes it through the tray's `IconThemePath`. Panels (GNOME, KDE, Waybar, etc.) resolve the engine's `icon` name against these bundled icons automatically. No system-wide installation is required, but this is **not** a replacement for the standard XDG path when a package manager is involved.

## Source Layout for Engine Developers

When implementing an engine, keep icon sources under a predictable resource directory in your repository:

```text
typio-engine-myengine/
├── src/
├── data/
│   └── icons/
│       └── hicolor/
│           └── scalable/
│               └── apps/
│                   ├── typio-engine-myengine.svg
│                   └── typio-engine-myengine-symbolic.svg
├── meson.build
└── ...
```

Your build system then installs them to either the system icon directory or the engine's bundled `icons/` directory. Example with Meson:

```meson
# Install to standard XDG path (recommended for packaged engines)
icon_scalable = files('data/icons/hicolor/scalable/apps/typio-engine-myengine.svg')
icon_symbolic = files('data/icons/hicolor/scalable/apps/typio-engine-myengine-symbolic.svg')

install_data(icon_scalable,
    install_dir: get_option('datadir') / 'icons' / 'hicolor' / 'scalable' / 'apps'
)
install_data(icon_symbolic,
    install_dir: get_option('datadir') / 'icons' / 'hicolor' / 'scalable' / 'apps'
)
```

## Prohibited Values

The framework rejects (logs a warning and drops) icons that match any of the following:

| Pattern | Example | Reason |
|---------|---------|--------|
| Empty or whitespace-only | `""`, `"  "` | Meaningless; silently ignored. |
| URL scheme | `"http://…"`, `"https://…"`, `"file://…"` | Security: no remote or URI resolution at runtime. |
| Relative path | `"./icon.svg"`, `"../icons/icon.svg"` | Security: prevents directory traversal. |
| Home directory reference | `"~/.local/…"` | Non-portable; expansion is host-dependent. |
| Path with `..` component | `"/usr/share/../foo.svg"` | Security: normalized form still rejected. |

## Deprecation Notice

**Absolute file paths are deprecated.** They were a stopgap for engines that lacked proper icon theme integration. Modern hosts aggregate engine icons automatically from `<engine-dir>/icons/` and resolve standard icon theme names. Engines using absolute paths should migrate to:

1. Pick a stable freedesktop icon name (e.g., `"typio-engine-<name>"`).
2. Ship the actual icon files:
   - **Packaged engines**: install to `${datadir}/icons/hicolor/…`.
   - **Bundled engines**: ship under `<engine-dir>/icons/hicolor/`.
3. Set `TypioEngineInfo.icon` to the freedesktop name.

## Runtime Behaviour

- **Valid icon** → retained on the engine's `TypioEngineInfo` and returned by `typio_registry_get_engine_icon()`.
- **Invalid icon** → logged as a warning, stored as `NULL`. The host should fall back to a generic input-method icon.
- **No icon provided** (`icon == NULL`) → treated as absent; host uses the generic fallback.
- **Deprecated absolute path** → accepted but emits a deprecation warning. Will be rejected in a future release.

## Mode Icons vs Engine Icons

Do not confuse the two:

| Level | Field | Purpose | Updated |
|-------|-------|---------|---------|
| **Engine** | `TypioEngineInfo.icon` | Static brand icon for engine lists and settings. | Never (set at compile time). |
| **Mode** | `TypioKeyboardEngineMode.icon_name` | Dynamic tray icon reflecting current input state (e.g. Hiragana vs ASCII). | Every time `get_active_mode` is called. |

Mode icons follow the same freedesktop-name rules but are not validated by the framework; the host decides how to resolve them.
