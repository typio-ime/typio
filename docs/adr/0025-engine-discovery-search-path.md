---
id: ADR-0025
title: "Engine discovery — ordered search path, no user-level auto-scan"
status: accepted
date: 2026-06-05
scope: engine
---

# ADR-0025: Engine discovery — ordered search path, no user-level auto-scan

- **Status**: Accepted
- **Date**: 2026-06-05
- **Deciders**: Project maintainers

## Context

`typio_engine_dirs_build` (`src/plugin_loader.c`) assembles the engine search
list from at most three single-valued sources: one `--engine-dir`/`-E`
override, one `$TYPIO_ENGINE_DIR`, and the compile-time system directory
`<libdir>/typio/engines`. The loader registers the first engine of each name
it finds and ignores any file not matching `libtypio_engine_<name>.so`.

Two problems motivate a redesign:

1. **Single-valued override and env var.** A developer working on more than
   one engine repository cannot point the daemon at several build directories;
   `docs/dev/setup.md` tells them to *symlink* engines into one directory as a
   workaround. Every other Unix search mechanism — `PATH`, `LD_LIBRARY_PATH`,
   `XDG_DATA_DIRS`, `GST_PLUGIN_PATH` — is a colon-separated, ordered, multi-
   entry list. The single-valued `TYPIO_ENGINE_DIR` is the outlier.

2. **Whether to auto-scan a per-user directory.** A per-user default
   (`~/.local/lib/...` or `$XDG_DATA_HOME/typio/engines`) would let a user drop
   in an engine and have it load with no flag. But engines are native `.so`
   files `dlopen`ed into the daemon address space, and the daemon observes
   every keystroke across every application. Auto-loading from a user-writable
   directory is a code-injection surface into a keystroke-privileged process.

## Decision

Engine discovery is an **ordered search path**. The loader registers the first
engine of each name and logs every shadowed duplicate. Sources, highest to
lowest precedence:

1. `--engine-dir DIR` — **repeatable**; each occurrence accumulates in the
   order given.
2. `$TYPIO_ENGINE_PATH` — **colon-separated list**, in listed order. Replaces
   the single-valued `TYPIO_ENGINE_DIR`, which is removed (no alias, per the
   ADR-0008 no-shim mandate).
3. The compile-time system directory `<libdir>/typio/engines`.

**No user-level or `$HOME` auto-scan.** The daemon auto-loads only from the
trusted, root-owned, package-managed system directory. Every other source is
an explicit operator opt-in (a flag or an exported list). A rootless,
Nix, or Flatpak user adds one line to their profile
(`export TYPIO_ENGINE_PATH=…`) or passes `--engine-dir`; that line *is* the
trust decision, recorded where it can be audited.

Packaging is unchanged: `libtypio_engine_<name>.so`, with the modality
discovered at load time via `typio_engine_get_info()`. Discovery stays
modality-unified (see [ADR-0026](0026-modality-explicit-engine-control-surface.md)).

## Alternatives considered

- **Auto-scan `$XDG_DATA_HOME/typio/engines`.** Rejected. Even though `share`
  (not `lib`) is XDG-correct and the GStreamer precedent puts binary plugins
  there, it reintroduces silent loading of user-writable native code into a
  keystroke-privileged process. The convenience payoff is small: engines ship
  as distribution packages into the system directory, and the people who
  hand-install are developers and rootless users who can set an env var.
- **Auto-scan `~/.local/lib/typio/engines`.** Rejected twice over: `~/.local/lib`
  is not part of the XDG Base Directory specification, and it carries the same
  code-injection concern.
- **Keep the single-valued override and the symlink workaround.** Rejected.
  Every Unix search path is a list; the symlink hack breaks the moment two
  repositories need different builds of the same engine.
- **Keep `TYPIO_ENGINE_DIR` as a deprecated alias.** Rejected per the ADR-0008
  greenfield, no-backward-compatibility mandate; in-tree callers move in the
  same wave.

## Consequences

- Positive: multi-engine development is first-class —
  `--engine-dir ../typio-engine-mozc/build --engine-dir ../typio-engine-rime/build` —
  with no symlink hack and no copy step (each engine's build directory already
  contains its `libtypio_engine_<name>.so`).
- Positive: the environment variable follows the universal `*_PATH` idiom.
- Positive: a conservative trust boundary — auto-load only from the system
  directory — is a defensible security posture for a keystroke-privileged
  daemon, mirroring how `sudo`, `ssh`, and systemd *system* units decline to
  trust `$HOME`.
- Trade-off: per-user installs need an explicit `TYPIO_ENGINE_PATH` entry;
  there is no zero-config drop-in. Accepted as a feature (explicit trust), not
  a wart.
- Trade-off: `dirs_build` gains list parsing, dedup-by-name, and shadow
  logging; `cli.c` accumulates repeated `-E` into a list. Both bounded.
- Negative (accepted): scripts or users relying on `TYPIO_ENGINE_DIR` must
  rename it to `TYPIO_ENGINE_PATH`.

## Related

- [ADR-0026](0026-modality-explicit-engine-control-surface.md) — modality is
  explicit at the control surface but unified at discovery.
- [ADR-0008](0008-ipc-protocol-resource-namespaces-uds-only.md) — the no-shim,
  greenfield mandate that forbids the `TYPIO_ENGINE_DIR` alias.
- `docs/reference/engine-discovery.md` and `docs/dev/setup.md` — updated to the
  new precedence and the repeatable flag.
