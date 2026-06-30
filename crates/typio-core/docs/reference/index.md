# Reference

Lookup-oriented documentation. These pages are dense, complete, and accurate — use them when you already know what you are looking for.

## C ABI Reference

- [Host ABI](host-abi/index.md) — library ABI of `libtypio.so` (instance, registry, input context, config, events, logging)
- [Engine](engine/index.md) — contract that engine worker implementations use (entry points, types, ops)

## Interfaces and Configuration

- [CLI Reference](cli.md) — Command-line flags and `typio` client subcommands
- [Configuration Reference](configuration.md) — All `core.toml` keys, types, and defaults
- [Installation Layout Reference](install-layout.md) — Every file and directory created by `cargo build --release` and manual install
- [Engine Reference](engines.md) — keyboard and voice engine categories, config keys, capabilities, and engine ABI
- [Engine Protocol Reference](engine-protocol.md) — fd 3 framed protocol between libtypio and engine processes
- [Engine Icon Reference](engine/icons.md) — Valid formats, prohibited values, and host responsibilities
- [Package for Distribution](package-for-distribution.md) — Install paths, pkg-config dependencies, and packaging examples for engine authors and distro packagers

## Glossary

- [Glossary](glossary.md) — Definitions of preedit, commit, grab, engine session, property bag, and other Typio-specific terms

## Looking for something else?

- Learning the basics? See [Tutorials](../tutorials/)
- Trying to accomplish a task? See [How-to guides](../how-to/)
- Want to understand the design? See [Explanation](../explanation/)
