# Contributing

Typio is a Wayland input-method host. Contributions are welcome as pull
requests against `main`; every change must build, pass the test suites in
[docs/dev/testing.md](docs/dev/testing.md), and leave the documentation it
touches accurate.

## Build and test

Follow [Developer Setup](docs/dev/setup.md) for the native prerequisites and
the current build commands — including the sibling `optics`/`flux` build that
the daemon links against. [Testing](docs/dev/testing.md) lists the workspace
test suites and their commands. Do not copy those commands into other files:
the setup and testing guides are their single source.

## Changes that need more than code

- **Architectural decisions** get an ADR in `docs/adr/` when the change clears
  the significance threshold. Follow [Architectural Decision
  Records](docs/dev/standards/adr.md) and
  register the record in [the ADR index](docs/adr/index.md).
- **User-visible changes** get an entry under **Unreleased** in
  `CHANGELOG.md` ([Keep a Changelog](https://keepachangelog.com/) format).
- **Breaking changes to an external interface** must respect its tier in the
  [Interface Stability Reference](docs/reference/stability.md).
- **Documentation** follows the engineering standards in
  [docs/dev/standards/](docs/dev/standards/README.md).
  Route new content through the [taxonomy](docs/dev/standards/taxonomy.md)
  and write it with the [style guide](docs/dev/standards/style.md).
  Run `tools/check-docs.sh` before pushing: it enforces the invariants that
  review would otherwise catch by hand.
- **Code style** is described in [Code Style](docs/dev/code-style.md).

## Repository governance

Commit, tag, versioning, and release conventions are charters, not folklore:
see [Engineering Workflow](docs/dev/standards/workflow.md). Read it before cutting a
release or rewriting published history.

## Tests

[docs/dev/testing.md](docs/dev/testing.md) lists the subsystems that require
test updates when touched, and the test-ownership rules for the daemon, the
runtime, the engine crates, and the CLI.

## Security

Do not report vulnerabilities in public issues; see the
[Security Model](docs/explanation/security-model.md).
