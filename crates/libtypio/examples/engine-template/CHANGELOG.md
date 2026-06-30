# Changelog

## Unreleased

## v0.1.1 - 2026-06-19

- `worker_main.c` is now byte-identical to the shared harness shipped by
  the other engine workers. Picking up the salience field on
  MODE/ACTIVE_MODE lines, ACTIVE_MODE emission after every mode-affecting
  request, and the safer teardown ordering (deactivate → instance →
  engine) happens automatically when forking the template.

## v0.1.0 - 2026-06-06

- Initial release: minimal hello-world engine that commits the literal
  string "hello" when the `a` key is pressed, intended as the starting
  point for new engine authors.
