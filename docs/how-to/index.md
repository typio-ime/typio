# How-to Guides

Charter: this directory holds **task-oriented** pages. Each guide solves one
concrete problem for someone who already knows what they want. Headings state
the task; steps start with imperative verbs. If you are learning the system from
zero, start with the [Tutorial](../tutorials/index.md) instead.

| Guide | Solves |
| :--- | :--- |
| [How to Package for Distribution](package-for-distribution.md) | Producing installable binaries, systemd units, icons, and engine manifests |
| [How to Configure Typio Graphically](configure-graphically.md) | Changing settings through `typio-settings` rather than by hand-editing files |
| [How to Diagnose Candidate-Switching Lag](diagnose-candidate-lag.md) | Narrowing down slow or stuttering candidate switching |
| [How to Communicate with Typio over UDS](communicate-over-uds.md) | Writing a client against the TIP control socket |
| [How to Write an Engine](write-an-engine.md) | Building a protocol worker that the daemon can discover and drive |
| [Troubleshooting](troubleshooting.md) | Diagnosing a daemon that will not start, shows no candidates, or misbehaves |

## See also

- [Reference](../reference/index.md) — exact keys, verbs, and wire formats used by these guides
- [Architecture Blueprints](../architecture/index.md) — how each subsystem works today
