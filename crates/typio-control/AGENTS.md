# Agent guidelines: typio-control

This crate holds the `typioctl` control client. It is a workspace member, so
repository-level rules apply here first: read [AGENTS.md](../../AGENTS.md) and
[Engineering Standards](../../docs/dev/standards/README.md) before changing
behaviour, and treat the control protocol in
[docs/reference/ipc-protocol.md](../../docs/reference/ipc-protocol.md) as the
contract this crate consumes.

Before writing, modifying, or archiving any documentation, read and follow the
documentation standards at
`../../docs/dev/standards/style.md`. Routing — which page new
content belongs on — lives in
`../../docs/dev/standards/taxonomy.md`.
