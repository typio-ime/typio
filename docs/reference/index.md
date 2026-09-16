# Reference

Charter: this directory holds **lookup-oriented** pages. A reference page states
the current, exact truth about an external surface — commands, keys, wire
formats, tiers — with austere prose and tables. It does not explain rationale
(see [Explanation](../explanation/index.md)) or walk through a task (see
[How-to Guides](../how-to/index.md)).

Reference pages describe interfaces other programs and users depend on. If a
page here disagrees with the code, the page is wrong and the pull request that
changed the code owns the fix.

| Page | Lookup for |
| :--- | :--- |
| [Command-Line Interface Reference](cli.md) | `typio`, `typioctl`, and `typio-settings` command lines, flags, subcommand to RPC mapping, log levels, signals |
| [Configuration Reference](configuration.md) | `core.toml` and `platform.toml`: keys, defaults, ownership, reload behaviour |
| [IPC Protocol Reference (TIP v3)](ipc-protocol.md) | The UDS JSON-RPC control surface: framing, methods, events, error codes |
| [Engine Protocol Reference](engine-protocol.md) | The manifest-declared worker contract: framing, handshake, requests, records, limits |
| [Engine Discovery Reference](engine-discovery.md) | Where manifests are searched, file-name rules, precedence, icons |
| [Interface Stability Reference](stability.md) | The stability tier of every external interface |
| [Glossary](glossary.md) | Canonical project terms, with the page that owns each definition |

## See also

- [How to Communicate with Typio over UDS](../how-to/communicate-over-uds.md) — the same control surface as a worked recipe
- [Architecture Blueprint: Control Plane and Clients](../architecture/control-and-clients.md) — how these surfaces are implemented today
