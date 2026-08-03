# Contributing

## Build and test

Follow [Developer Setup](docs/dev/setup.md) for native dependencies, then:

```bash
meson compile -C ../optics/build
export FLUX_BUILD_DIR="$PWD/../optics/build"
export FLUX_SOURCE_DIR="$PWD/../optics/libs/flux"
cargo build --release -p typio-host --bin typio
cargo test -p typio-host -p typio-core \
  -p typio-engine-protocol -p typio-engine-manifest -p typio-vet
```

Before sending a change, run the relevant suites described in
[Testing](docs/dev/testing.md). The runtime, typed engine protocol, manifest
parser, and conformance tool live in this workspace and change atomically.
`flux` remains a sibling native build prerequisite.

## Changes that need more than code

- **Architectural decisions** get an ADR. Follow the
  [ADR workflow](docs/dev/documentation/adr-workflow.md).
- **User-visible changes** get an entry under **Unreleased** in
  `CHANGELOG.md` ([Keep a Changelog](https://keepachangelog.com/) format).
- **Breaking changes to an external interface** must respect its tier in
  the [Interface Stability Reference](docs/reference/stability.md).
- **Documentation** follows the
  [documentation governance](docs/dev/documentation/index.md) rules; read
  the routing and style guide pages before adding or moving a doc.
- **Code style** is described in [Code Style](docs/dev/code-style.md).

## Tests

`docs/dev/testing.md` lists the subsystems that require test updates when
touched. New parsers of external input also need a fuzz harness (see the
Fuzzing section there).

## Security

Do not report vulnerabilities in public issues; see
[SECURITY.md](SECURITY.md).
