# typio-core documentation

`typio-core` is the platform-neutral Rust runtime embedded by the Typio daemon.
It owns configuration, engine registry policy, language and mode selection,
input-context state, and process-backend lifecycle. It builds only an `rlib`;
it does not publish a C ABI or load engine code into the daemon.

Start with:

- [Architecture overview](explanation/architecture-overview.md)
- [Engine contract](explanation/engine-contract.md)
- [Composition state machine](explanation/composition-state-machine.md)
- [Engine Protocol reference](reference/engine-protocol.md)
- [Engine author path](engine-author.md)
- [Architecture decisions](adr/)

Product setup, configuration, packaging, TIP, Wayland, and contributor
documentation lives in the repository-level [`docs/`](../../../docs/) tree.
