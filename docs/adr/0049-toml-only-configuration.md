# ADR-0049: TOML-only configuration

- **Status**: Accepted
- **Date**: 2026-09-11
- **Deciders**: Core Maintainers

## Context

`Config::parse` accepted configuration text in two dialects: TOML, and a
hand-written "INI-like" fallback (`config/parse.rs::parse_ini_like`) tried only
when the TOML parse failed. The fallback was a leftover from the retired C
implementation, which had no TOML library and read a `key = value` dialect of
its own.

The fallback was actively harmful:

- **It masked malformed files.** A typo that made a TOML document unparseable
  could still "succeed" through the INI path, producing a partially-populated
  configuration tree instead of a parse error. Callers that rely on
  `ConfigError::Parse` to retain the last known-good state saw a silently
  degraded config instead.
- **It was undocumented.** No user-facing document, example file, or test
  claimed INI support. The only record of the feature was the code itself and
  the `ConfigError::Parse` doc comment.
- **It doubled the accepted grammar.** Two dialects meant two sets of edge
  cases (quoting, arrays, booleans, sections) to reason about for a format
  nothing in the project writes.

## Decision

Configuration is TOML, and only TOML.

- Delete `parse_ini_like` and its value parser from `config/parse.rs`.
- `Config::parse` returns `ConfigError::Parse` for any input that is not valid
  TOML. There is no fallback dialect.
- Keep `Config::save`/`to_toml` as the only writer, so round-trips are
  single-format.
- Cover the removal with a regression test asserting that a legacy
  `key = value` document is rejected rather than accepted.

## Alternatives considered

- **Keep the fallback for one release with a deprecation warning.** Rejected:
  the project mandate is a greenfield rewrite with no compatibility shims, and
  no user-facing document ever promised the dialect. A warning would advertise
  a format nobody asked for while preserving both grammars.
- **Accept INI only when an explicit opt-in key is present.** Rejected: adds a
  second configuration surface to maintain, and the problem it solves (reading
  a pre-Rust file) applies only to installs that predate the Rust daemon.
- **Migrate legacy files automatically on load.** Rejected: silently rewriting
  user configuration is a worse failure mode than a clear parse error.

## Consequences

- Positive: a malformed configuration is always reported as a parse error, so
  the "retain last known-good config" behaviour actually triggers.
- Positive: one accepted grammar, one writer, one set of edge cases.
- Positive: removes ~80 lines of unreachable-in-practice parsing code.
- Negative (accepted): a pre-Rust `core.toml` in the old INI dialect now fails
  to load with a parse error instead of being coerced. Such a file must be
  rewritten as TOML; `data/core.toml.example` is the reference.
