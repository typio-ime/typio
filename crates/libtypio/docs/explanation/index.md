# Explanation

Understanding-oriented documents that explain *why* Typio works the way it does. These are discursive and may include opinions and trade-offs.

- [Architecture Overview](architecture-overview.md) — High-level structure, components, data flow, and design rules
- [Crate Organization and the ABI Split](crate-organization.md) — Why the ecosystem is split into multiple crates and repos, the principles behind each boundary, and the ABI's mission as the sole cross-repo contract
- [Composition State Machine](composition-state-machine.md) — Abstract key-to-commit pipeline: preedit, candidates, the four `process_key` outcomes, and focus/reset semantics
- [Engine Contract](engine-contract.md) — Why the framework is language-agnostic: the engine↔framework boundary, the two-emits contract (composition + commit), property-bag state, and how Rime, Mozc, compose, and voice all share the same interface
- [Engine → Host Resource Flow](engine-host-resource-flow.md) — The five channels engines use to publish data (icons, modes, status, properties, activation) to the host; ownership, lifetimes, and validation policy
- [Config & Runtime Ownership](config-runtime-ownership.md) — Who owns persisted config, runtime state, staged edits, and view state across daemon and control surfaces
- [Configuration System](configuration-system.md) — Why the schema table is the single source of truth for all config fields
- [Voice Input Architecture](voice-input.md) — State machine, backend proxy pattern, audio pipeline, and reload semantics
- [Modifier Key Consumption](modifier-key-consumption.md) — Why handled modifier keys must not be forwarded to the client application

## Looking for something else?

- Learning the basics? See [Tutorials](../tutorials/)
- Trying to accomplish a task? See [How-to guides](../how-to/)
- Looking up a value? See [Reference](../reference/)
- Want to see why a specific decision was made? See [ADR](../adr/)
