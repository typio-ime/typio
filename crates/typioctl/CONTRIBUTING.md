# Contributing

Thank you for your interest in improving `typioctl`.

Every change should keep three things aligned:

- code
- tests
- docs

If one of those changes and the others do not, the patch is incomplete.

## Quick start

```bash
cargo build -p typioctl
cargo test -p typioctl
```

## Developer documentation

- [Developer Setup](docs/dev/setup.md)
- [Testing](docs/dev/testing.md)
- [Code Style](docs/dev/code-style.md)
- [Project Layout](docs/dev/project-layout.md)

## Pull Request Checklist

- [ ] Build succeeds from a clean tree (`cargo build -p typioctl`)
- [ ] Tests pass (`cargo test -p typioctl`)
- [ ] User-facing behavior is documented
- [ ] `CHANGELOG.md` is updated
- [ ] If architectural change: ADR added

## Questions?

Open an issue or discussion on the Typio project repository.
