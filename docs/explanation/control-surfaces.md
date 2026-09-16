# Control Surfaces

Typio has one external control transport and two in-process presentation
surfaces. Runtime policy has one owner regardless of where an action begins.

Two entry paths reach that owner:

1. An external client — the command-line client or the settings application —
   sends a request over the control socket. The socket server decodes it, hands
   it to a transport-agnostic dispatch layer, and that layer performs an owned
   operation on the runtime.
2. A user acts on the tray. The callback enqueues a typed daemon event that the
   main event loop applies to the same runtime.

Both paths end in the runtime. Its state controller produces a snapshot, which
flows back to the socket server as responses and notifications and refreshes the
tray from the same source. Desktop notifications are one-way output and take no
part in this loop.

| Surface | Transport | Role |
|---|---|---|
| TIP | credential-checked Unix socket, length-prefixed JSON-RPC | external query, mutation, and subscriptions |
| System tray | SNI/dbusmenu over the session bus | in-process presentation and user actions |
| Notifications | the desktop notification service | one-way health output |

There is no D-Bus control API. D-Bus is used only for desktop integration.
There is also no engine control socket exposed to clients: the engine's private
descriptor belongs to one private daemon/engine relationship and carries Typio
Engine Protocol, not TIP.

## TIP Request Flow

One request travels through the stack in this order:

1. **Client** — sends a length-prefixed JSON-RPC request over the Unix socket.
2. **Socket server** — reads and decodes the frame, applies the peer and limit
   checks, and passes the decoded method and parameters to the dispatcher.
3. **Status service** — performs the owned runtime operation and returns a typed
   result.
4. **Socket server** — frames the typed result as a JSON-RPC response.
5. **Client** — receives the length-prefixed response.

The dispatch layer is transport-agnostic and testable without a socket, behind a
backend boundary. The production backend borrows the main-loop-owned runtime
instance; it does not hold raw pointers. Socket framing, method dispatch, and
runtime policy remain separate concerns.

Clients subscribe to an event topic list over the same socket. Registry,
language, mode, and config changes refresh the state snapshot; the notification
bus sends notifications to subscribed peers, and the tray refreshes from the same
state. Peer uid checks, the frame cap, and connection limits are enforced before
dispatch.

## Tray Actions

Tray callbacks never mutate the registry from the D-Bus worker. They enqueue a
typed tray daemon event; the main event loop applies the language, engine,
restart, or quit operation and then refreshes all state surfaces. This keeps
runtime ownership on the single-threaded main loop.

## Failure Rule

External clients must tolerate the daemon being absent, reconnect after socket
replacement, negotiate the protocol version during the handshake, and re-read
state after subscribing. A client cache is a view, never a second source of
truth.

## See Also

- [TIP Reference](../reference/ipc-protocol.md) — methods, event topics, and framing.
- [Control Plane and Clients Blueprint](../architecture/control-and-clients.md) —
  the current socket server, dispatch layer, clients, and tray implementation.
