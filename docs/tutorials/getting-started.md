# Your First Typio Session

This tutorial takes you from a machine with no Typio installed to typing with an
engine in a real application. You will build or install the daemon, start it as
a user service, install at least one engine, and switch input method from the
keyboard.

Typio is a Wayland-native input method host. It requires a Wayland session
(`text-input-v2` on the application side, `input-method-v2` on the host side);
X11 is not supported.

## 1. Get Typio onto the machine

Typio builds from source against a sibling Optics checkout. Either follow the
quick start in [README.md](../../README.md), which builds the daemon, the
`typioctl` client, and the settings application, or follow
[How to Package for Distribution](../how-to/package-for-distribution.md) if you
want installable binaries, a systemd unit, and desktop metadata.

**Expected outcome:** `target/release/typio`, `target/release/typioctl`, and
`target/release/typio-settings` exist, or the same three binaries are installed
under the prefix you staged.

## 2. Start the daemon as a user service

Installed packages start the daemon through the systemd user service, which is
the only supported startup surface:

```bash
systemctl --user enable --now typio.service
journalctl --user -u typio -f
```

If you are running from a build tree instead, start `typio` in a terminal with
`--verbose` and keep it in the foreground.

**Expected outcome:** the service starts without an error, and the journal shows
the daemon reporting its version and protocol version.

## 3. Confirm the daemon is answering

In a second terminal, ask the control client for the daemon status:

```bash
typioctl daemon status
```

**Expected outcome:** a status report showing the daemon version, the control
protocol version, uptime, and the active keyboard engine. If the client cannot
connect, the daemon is not running or is listening on a different socket —
see [Troubleshooting](../how-to/troubleshooting.md).

## 4. Install an engine

Typio ships no engines. An engine is a separate package: a manifest plus a
worker executable, discovered from the system engine directory. Install one
engine package, or build an engine yourself —
[How to Write an Engine](../how-to/write-an-engine.md) walks through it.

**Expected outcome:** at least one `typio-engine-*.toml` manifest exists in the
engine directory, and `typioctl engine list` shows that engine. The
manifest search path and file-name rules are in the
[Engine Discovery Reference](../reference/engine-discovery.md).

## 5. Select the engine and its language

List what is available, then activate an engine:

```bash
typioctl engine list
typioctl engine use <engine-name>
typioctl language list
```

**Expected outcome:** `engine list` marks the active engine with a `*`, and
`language list` shows the languages the engine declares.

## 6. Type with it

Open any application with a text field — a browser address bar, a terminal, an
editor — click into the field, and start typing.

Press `Ctrl+Shift` to switch language, which is Typio's default binding (the
`[shortcuts]` section of the configuration file changes it). Pressing it cycles
to the next language; if the active engine declares no languages, it cycles to
the next engine instead.

**Expected outcome:** typing produces text as usual, and pressing `Ctrl+Shift`
shows the candidate Panel next to the caret when the active engine has
candidates to offer. The indicator in the system tray shows the active language.

## 7. Adjust it

Run the graphical settings application to change appearance, language order, and
shortcuts:

```bash
typio-settings
```

**Expected outcome:** the settings window lists the running daemon's engines and
languages, and changes take effect without restarting the daemon. For the
underlying keys, see the [Configuration Reference](../reference/configuration.md)
and [How to Configure Typio Graphically](../how-to/configure-graphically.md).

## Where to go next

- Nothing appears on screen when you type — [Troubleshooting](../how-to/troubleshooting.md).
- Candidates switch slowly — [How to Diagnose Candidate-Switching Lag](../how-to/diagnose-candidate-lag.md).
- You want to understand the design — [Explanation](../explanation/index.md).
- You want to build an engine — [How to Write an Engine](../how-to/write-an-engine.md).
