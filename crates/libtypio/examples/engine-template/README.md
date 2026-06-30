# typio-engine-template

A minimal hello-world keyboard engine for the
[Typio](https://github.com/) input method framework.  Use it as a starting
point for writing your own engine.

It builds to the `typio-engine-hello` worker executable plus a
`typio-engine-hello.toml` manifest.  A Typio host discovers the manifest,
spawns the worker, and talks to it over the fd-3 Typio Engine Protocol.

## What it does

`process_key` intercepts the letter `a` and commits `hello`.  Everything
else passes through.  No preedit, no candidate window, no configuration —
just enough plumbing to demonstrate the engine ABI.

The split between the two source files mirrors every C engine in the
Typio family:

| File | Role |
|---|---|
| `src/hello_engine.c` | Your engine: classic ABI vtables (`TypioEngineBaseOps`, `TypioKeyboardEngineOps`) and metadata. |
| `src/worker_main.c` | Generic harness: wraps the vtables in the fd-3 engine protocol.  Copy it unchanged. |

## Building

Requires the libtypio engine ABI (provides `typio-engine-abi.pc`). If
libtypio is not installed system-wide, point pkg-config at a local
`typio` workspace build:

```sh
PKG_CONFIG_PATH=/path/to/typio/target/release meson setup build
ninja -C build
```

### Local debugging install

```sh
mkdir -p ~/.local/share/typio/engines
cp build/typio-engine-hello build/typio-engine-hello.toml \
   ~/.local/share/typio/engines/
```

The development manifest's `command` is `./typio-engine-hello`, resolved
relative to the manifest, so copying both files together is enough.
Restart the Typio host; `typioctl engine list` should now show the
`hello` engine.

## ABI

This template targets the Typio engine ABI version stamped at build time
and exported via the `typio_engine_abi_version` symbol.  The host refuses
to load an engine with a different ABI major or a newer ABI minor.
Pre-1.0: rebuild against each libtypio release.

## Discovery & install paths

A Typio host finds engines by scanning directories for
`typio-engine-*.toml` manifests in priority order; the **first** engine of
a given name wins, so a user-dir install shadows a system one (handy for
testing a patched build):

| Order | Source | Path |
|---|---|---|
| 1 | `--engine-dir DIR` | host command line |
| 2 | `$TYPIO_ENGINE_DIR` | environment variable |
| 3 | **User** | `$XDG_DATA_HOME/typio/engines` (default `~/.local/share/typio/engines`) |
| 4 | **System** | `<datadir>/typio/engines` (e.g. `/usr/share/typio/engines`) |

The manifest's `command` names the worker executable (installed to
`<libexecdir>/typio/engines` by this build); `icon` names a freedesktop
icon installed into the hicolor theme.

## Next steps

To turn this into a real engine, edit `src/hello_engine.c` and
`typio-engine-hello.toml.in` (leave `src/worker_main.c` alone):

1. **Rename**: change the `name` field in `TypioEngineInfo` and the
   manifest, and rename the project in `meson.build`.
2. **Declare languages**: list the BCP-47 tags your engine supports in the
   manifest's `languages` key (primary first).  The language is the
   user-facing switch unit — the framework picks your engine when one of
   these tags is activated.  Use `["mul"]` for an any-language engine.
3. **Declare capabilities**: list `required` capabilities (host must
   provide) and `optional` ones (best-effort) in both the manifest and
   `TypioEngineInfo`.  See `typio/abi/types.h` in libtypio for the
   standard set.
4. **Implement `process_key`**: see `typio/abi/engine.h` for the full
   `TypioKeyboardEngineOps` vtable.  `init` / `destroy` / `focus_in` /
   `focus_out` / `reset` / `reload_config` are all mandatory; supply
   no-ops where unused.
5. **(Optional) Declare config fields**: expose runtime properties
   (schema selection, settings) by exporting `typio_engine_get_config_schema`
   so the host registers your `engines.<name>.*` keys at load time;
   react to changes via `on_config_change` on `TypioEngineBaseOps`.
6. **(Optional) Expose commands**: declare invokable actions
   (`deploy`, `reload-dict`, …) via `TypioEngineSurfaceOps`
   (`list_commands` / `invoke_command`).

See [`docs/how-to/create-custom-keyboard-engine.md`](../../docs/how-to/create-custom-keyboard-engine.md)
for the full guide.

## License

See [LICENSE](LICENSE).
