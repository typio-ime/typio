# Contributing to typio-core

`typio-core` is maintained inside the Typio workspace. Follow the repository
[contribution guide](../../CONTRIBUTING.md),
[developer setup](../../docs/dev/setup.md), and
[testing guide](../../docs/dev/testing.md).

For runtime or engine-contract changes, run:

```bash
cargo test -p typio-core -p typio-engine-protocol \
  -p typio-engine-manifest -p typio-vet
```

Changes to the daemon/engine boundary must update the typed protocol crate,
the protocol reference, the runnable worker example, and `typio-vet` in the
same change. Do not add a dynamic-loader, header, or binary-layout shortcut.
