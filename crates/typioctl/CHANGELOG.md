# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-06-19

### Fixed

- `engine use <name>` and `engine next [--kind]` now dispatch to the
  modality-explicit `keyboard.use` / `voice.use` / `keyboard.next` /
  `voice.next` verbs introduced in TIP v2 (ADR-0026). The previous
  `engine.use` / `engine.next` calls were rejected as
  `-32601 method-not-found` by current daemons. `engine use` resolves
  the kind from `engine.list` so the CLI stays kind-agnostic; `engine
  next` defaults to the keyboard slot when no kind is given.

## [0.1.0] - 2026-06-13

### Added

- `language` resource (`list`, `use <tag>`, `next`, `prev`) mapping to the
  TIP v3 `language.*` verbs — the language-first switching surface
  (typio ADR-0031). Requires daemon protocolVersion ≥ 3.

## [0.0.1] - 2026-06-04

### Added

- Initial release.
