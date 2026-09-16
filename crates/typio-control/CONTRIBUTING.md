# Contributing

Thank you for your interest in improving `typioctl`.

Every change should keep three things aligned:

- code
- tests
- docs

If one of those changes and the others do not, the patch is incomplete.

## Quick start

```bash
cargo build -p typio-control
cargo test -p typio-control
```

## Developer documentation

- [Developer Setup](../../docs/dev/setup.md)
- [Testing](../../docs/dev/testing.md)
- [Code Style](../../docs/dev/code-style.md)
- [Module Map](../../docs/dev/module-map.md)

## Pull Request Checklist

- [ ] Build succeeds from a clean tree (`cargo build -p typio-control`)
- [ ] Tests pass (`cargo test -p typio-control`)
- [ ] User-facing behavior is documented
- [ ] `CHANGELOG.md` is updated
- [ ] If architectural change: ADR added

## Questions?

Open an issue or discussion on the Typio project repository.
