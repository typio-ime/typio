# How to Communicate with Typio over UDS

The Typio daemon speaks **TIP v1** — JSON-RPC 2.0 with length-prefixed frames
over a Unix Domain Socket. This guide shows how to send raw requests; see
[Daemon IPC Protocol Reference](../../../../docs/reference/ipc-protocol.md)
for the complete method catalog.

## Prerequisites

- The Typio daemon (`typio`) is running.
- You know the socket path: `$XDG_RUNTIME_DIR/typio/daemon.sock`.

## Wire format

```
[ 4 bytes: payload length (big-endian uint32) ]
[ N bytes: UTF-8 JSON payload                 ]
```

Used for requests, responses, and server→client event notifications. Max
frame size: 1 MiB.

## Minimal client (Python)

```python
#!/usr/bin/env python3
import os, socket, struct, json

SOCK = os.path.expandvars("$XDG_RUNTIME_DIR/typio/daemon.sock")
_id = 0

def tip_call(method, params=None):
    global _id
    _id += 1
    payload = {"jsonrpc": "2.0", "id": _id, "method": method,
               "params": params or {}}
    data = json.dumps(payload).encode()
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCK)
    s.sendall(struct.pack(">I", len(data)) + data)
    length = struct.unpack(">I", s.recv(4))[0]
    resp = json.loads(s.recv(length))
    s.close()
    if "error" in resp:
        raise RuntimeError(resp["error"]["message"])
    return resp["result"]

# Handshake
print(tip_call("hello"))

# Switch engine
tip_call("engine.use", {"name": "rime"})

# Set Rime schema (engine.set in CLI → config.set on the wire)
tip_call("config.set",
         {"key": "engines.rime.schema", "value": "luna_pinyin"})

# Run an engine command
tip_call("engine.invoke", {"name": "rime", "command": "deploy"})

# Status snapshot
print(tip_call("daemon.status"))
```

## Subscribing to events

The same connection can be used for both request/response calls and
push-based notifications.

```python
# Reuse one connection
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(SOCK)

def send(payload):
    data = json.dumps(payload).encode()
    s.sendall(struct.pack(">I", len(data)) + data)

def recv():
    length = struct.unpack(">I", s.recv(4))[0]
    return json.loads(s.recv(length))

send({"jsonrpc":"2.0","id":1,"method":"events.subscribe","params":{}})
print(recv())  # { "subscribed": true }

# Subsequent frames are JSON-RPC notifications:
#   {"jsonrpc":"2.0","method":"engine.changed",
#    "params":{"activeKeyboardEngine":"rime","activeVoiceEngine":""}}
while True:
    print(recv())
```

## Error handling

```json
{"jsonrpc":"2.0","id":7,"error":{"code":-32602,"message":"Unknown engine"}}
```

| Code | Meaning |
|---|---|
| `-32700` | Parse error |
| `-32600` | Invalid request |
| `-32601` | Method not supported |
| `-32602` | Invalid params |
| `-32603` | Internal error |

## See also

- [Daemon IPC Protocol Reference](../../../../docs/reference/ipc-protocol.md) — Complete method catalog.
- [CLI Reference](../reference/command.md) — `typioctl` command to RPC mapping.
