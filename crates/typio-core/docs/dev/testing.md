# Testing

This document is for contributors. It covers how to run and write tests.

## Run the test suite

```bash
cargo test
```

Run with verbose output to see individual test names:

```bash
cargo test -- --nocapture
```

Run a specific test or module:

```bash
cargo test engine_manager
cargo test e2e_register_mock_engine_and_process_key
```

Run with sanitizer coverage (requires nightly toolchain):

```bash
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test
```

## Test ownership

Add or update tests when changing:

- config parsing or schema metadata
- engine manager behavior
- input context commit/preedit semantics
- public APIs under `include/typio`

## Style

- Use Rust 2024 edition for all workspace Rust code.
- Use `#[cfg(test)]` inline unit tests; the old C test files under `tests/*.c` have been removed.
- Keep public API names in the `typio_*` / `Typio*` style already used by the repo.
- Prefer local helpers and direct data flow over broad abstractions.
- Document non-obvious behavior near complex state transitions.
- Keep generated protocol details behind narrow module boundaries.
