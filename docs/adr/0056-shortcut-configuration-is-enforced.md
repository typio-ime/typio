# ADR-0056: Shortcut configuration is enforced, not decorative

- **Status**: Accepted (implemented for `exit`, `voice_ptt`, `switch_language`, `summon_indicator`; `switch_keyboard_engine` is resolved and matched but has no cycle action)
- **Date**: 2026-10-29
- **Scope**: loop
- **Deciders**: Typio maintainers
- **Related RFC**: Not used — this repository does not operate an RFC workflow

## Context

`typio-runtime` declares a `[shortcuts]` configuration section with defaults, and
`docs/reference/configuration.md` documents it as user-facing:

| Key | Documented default | Documented behaviour |
| :--- | :--- | :--- |
| `shortcuts.switch_language` | `Ctrl+Shift` | Cycle the enabled language list |
| `shortcuts.switch_keyboard_engine` | *(unbound)* | Cycle to the next keyboard engine |
| `shortcuts.exit` | `Ctrl+Shift+Escape` | Emergency shutdown |
| `shortcuts.voice_ptt` | `Super+v` | Push-to-talk for voice input |
| `shortcuts.summon_indicator` | `Ctrl+Super+i` | Re-show the on-screen indicator |

The frontend does not read any of them. Measured:

- `shortcuts.*` appears in the daemon's source only in prose comments; there is no
  config read on the keystroke path.
- The push-to-talk chord is hardcoded in the router as "keysym is `V`/`v`, SUPER
  set, CTRL and ALT clear".
- `KEY_ESCAPE` is defined in `keyboard_policy.rs` and referenced nowhere, so the
  documented emergency exit has no trigger at all.

The consequence is a configuration table that accepts any value, displays it in
the settings UI, survives a reload — and changes nothing. A user who rebinds
push-to-talk away from `Super+V` (for example because their window manager
already owns that chord) keeps the old binding and has no way to discover why.

This is the same defect class ADR-0055 addresses for the engine gate: an artifact
that reports a capability it does not have. Settings that are displayed, parsed,
and persisted but never consulted are *worse* than absent settings, because they
convert a missing feature into a silent misconfiguration.

## Decision

1. **Every documented shortcut key must be consulted on the keystroke path.**
   `typio-daemon` resolves its shortcut bindings from `core.toml` at startup and
   again on the config-reload event, and the router matches against those
   bindings rather than against hardcoded keysyms and modifier masks.
2. **A binding that cannot be parsed is a startup error, not a silent fallback.**
   An unparseable shortcut string reports which key and which value failed. It
   does not quietly revert to the default, because "I rebound it and nothing
   happened" is precisely the failure being removed.
3. **No undocumented hardcoded chord may shadow a config key.** If a chord is
   built in, it is documented and configurable; if it is configurable, the
   hardcoded copy is deleted rather than left as a fallback.
4. **The settings UI reflects enforcement.** A shortcut field is shown only when
   the frontend consults it.

## Rationale

**Why not simply document the bindings as fixed.** The defaults are already
documented, the keys are already in the schema, and the settings application
already edits them. Retracting that is a larger, more confusing change than
honouring it, and it removes a capability users of similar input methods expect.

**Why a parse failure must be loud.** The current behaviour is the exact failure
mode being eliminated: an accepted-then-ignored value. Falling back to the
default reproduces it with extra steps. Naming the offending key and value makes
the mistake self-correcting.

**Why this is a separate record from ADR-0055.** ADR-0055 is about the engine
conformance gate owning its own assertions. This is about the frontend honouring
its configuration. They share a diagnosis — declared-but-not-enforced surfaces —
but the fixes are in different crates with different regression surfaces, and
bundling them would make neither reviewable.

## Rejected Alternatives

- **Remove the `[shortcuts]` section and document fixed bindings.** Rejected:
  retracts a documented capability to avoid implementing it, breaks the settings
  UI that edits these keys, and leaves users unable to resolve a chord conflict
  with their window manager.
- **Keep the hardcoded chords and mark the config keys "reserved".** Rejected:
  "reserved" in a user-facing configuration table is a lie with a footnote. It
  also invites the next contributor to believe the keys work.
- **Honour only `voice_ptt` now and file the rest.** Rejected as the stopping
  point: one enforced key beside four decorative ones is harder to reason about
  than either extreme, and the fix is shared — one parser and one binding set.
- **Parse shortcuts lazily at match time.** Rejected: it puts allocation and
  error handling on the keystroke path, which ADR-0048 bounds tightly. Bindings
  are resolved once per config generation.

## Consequences

- `typio-daemon` resolves `[shortcuts]` at startup and on every config reload,
  and the router matches key events against those bindings. The hardcoded
  `Super+V` test and the unused `KEY_ESCAPE` constant are gone.
- `exit` and `summon_indicator` gained handlers they never had. The documented
  `Ctrl+Shift+Escape` emergency exit was unreachable; it now shuts the daemon
  down, and `Ctrl+Super+i` re-shows the indicator through a gate-free path.
- The defaults now live in exactly one place (`Shortcut::default_text`), which
  the config schema reads. A test asserts those defaults still match this
  reference page, so doc drift becomes a test failure rather than a support
  question.
- An unparseable value is reported per key and leaves that shortcut unbound.
- **Known gap.** `switch_keyboard_engine` is parsed, matched, and honoured by the
  router, but the daemon has no "cycle to next keyboard engine" action to invoke;
  its documented default is unbound precisely because of that. Wiring it is
  follow-up work and does not affect the four shortcuts users actually bind.
- The shortcut module lives in `typio-host-types` (a leaf crate) so both the
  runtime's schema and the daemon's router share one definition. That added a
  `typio-runtime -> typio-host-types` edge, which is acyclic.
