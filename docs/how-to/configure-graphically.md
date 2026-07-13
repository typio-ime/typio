# How to Configure Typio Graphically

Start the Typio daemon, then launch the settings application:

```bash
systemctl --user start typio.service
typio-settings
```

Use **Input** to choose the active language, keyboard engine, or voice engine.
Engine-specific controls appear from the daemon's current config schema. Use
**Shortcuts** for accelerator strings and **Advanced** for the remaining
schema-backed fields.

Use **Appearance** for candidate Panel rendering and placement. These values
are saved to `$XDG_CONFIG_HOME/typio/platform.toml`, or
`~/.config/typio/platform.toml` when `XDG_CONFIG_HOME` is unset. Existing
comments and unknown keys are preserved.

The Input, Shortcuts, and Advanced pages require the daemon because they read
and write settings through TIP. The Appearance page remains available while
the daemon is stopped. If the application reports that the daemon is
unavailable, verify the connection from a terminal:

```bash
typioctl daemon status
```

See the [Configuration Reference](../reference/configuration.md) for every
config file and key.
