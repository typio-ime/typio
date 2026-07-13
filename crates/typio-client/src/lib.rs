//! Client for the Typio IPC Protocol (TIP).
//!
//! TIP uses JSON-RPC 2.0 over a Unix domain socket. Every JSON payload is
//! prefixed with a four-byte, big-endian length. This crate owns the shared
//! framing and response validation used by graphical and command-line clients.

use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

const TIP_MAX_FRAME: usize = 1 << 20;
const TIP_IO_TIMEOUT: Duration = Duration::from_secs(5);
const TIP_EVENT_TIMEOUT: Duration = Duration::from_millis(250);

/// Canonical daemon socket path.
pub fn socket_path() -> PathBuf {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(runtime_dir).join("typio/daemon.sock");
    }
    if let Some(home) = env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join(".local/share/typio/daemon.sock");
    }
    PathBuf::from("/tmp/typio-daemon.sock")
}

/// One server-pushed TIP notification.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

/// Synchronous JSON-RPC client over the TIP Unix socket.
pub struct Client {
    stream: UnixStream,
    next_id: i64,
}

impl Client {
    pub fn connect() -> io::Result<Self> {
        Self::connect_to(socket_path())
    }

    pub fn connect_to(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let stream = UnixStream::connect(path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot connect to {}: {error}; is the Typio daemon running?",
                    path.display()
                ),
            )
        })?;
        stream.set_read_timeout(Some(TIP_IO_TIMEOUT))?;
        stream.set_write_timeout(Some(TIP_IO_TIMEOUT))?;
        Ok(Self { stream, next_id: 1 })
    }

    /// Send a request and return its `result` value.
    pub fn call(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("TIP request id exhausted"))?;

        write_message(
            &mut self.stream,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }),
        )?;
        let response = read_message(&mut self.stream)?;
        validate_response(response, id)
    }

    /// Subscribe this connection to server-pushed events.
    ///
    /// An empty topic list subscribes to every event. The returned object owns
    /// the connection and cannot be used for ordinary request/response calls.
    pub fn subscribe(mut self, topics: &[&str]) -> io::Result<Subscription> {
        let params = if topics.is_empty() {
            json!({})
        } else {
            json!({ "topics": topics })
        };
        self.call("events.subscribe", params)?;
        self.stream.set_read_timeout(Some(TIP_EVENT_TIMEOUT))?;
        Ok(Subscription {
            stream: self.stream,
            buffer: Vec::with_capacity(4096),
        })
    }
}

/// Event-only TIP connection returned by [`Client::subscribe`].
pub struct Subscription {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl Subscription {
    /// Receive the next complete notification.
    ///
    /// Returns `Ok(None)` when no complete event arrives before the short
    /// event timeout. Partial frames remain buffered for the next call.
    pub fn recv(&mut self) -> io::Result<Option<Notification>> {
        if let Some(value) = take_buffered_message(&mut self.buffer)? {
            return parse_notification(value).map(Some);
        }

        let mut chunk = [0u8; 4096];
        match self.stream.read(&mut chunk) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "TIP event socket closed",
                ));
            }
            Ok(count) => self.buffer.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }

        take_buffered_message(&mut self.buffer)?
            .map(parse_notification)
            .transpose()
    }
}

fn write_message(stream: &mut UnixStream, value: &Value) -> io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(io::Error::other)?;
    if payload.len() > TIP_MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TIP request exceeds the 1 MiB frame limit",
        ));
    }
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()
}

fn read_message(stream: &mut UnixStream) -> io::Result<Value> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > TIP_MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TIP response exceeds the 1 MiB frame limit",
        ));
    }
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn take_buffered_message(buffer: &mut Vec<u8>) -> io::Result<Option<Value>> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let length = u32::from_be_bytes(buffer[..4].try_into().expect("four-byte prefix")) as usize;
    if length > TIP_MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TIP event exceeds the 1 MiB frame limit",
        ));
    }
    if buffer.len() < 4 + length {
        return Ok(None);
    }
    let payload = serde_json::from_slice(&buffer[4..4 + length])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    buffer.drain(..4 + length);
    Ok(Some(payload))
}

fn validate_response(response: Value, expected_id: i64) -> io::Result<Value> {
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid JSON-RPC version in TIP response",
        ));
    }
    if response.get("id").and_then(Value::as_i64) != Some(expected_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TIP response id mismatch",
        ));
    }
    match (response.get("result"), response.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown TIP error");
            Err(io::Error::other(message.to_owned()))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TIP response must contain exactly one of result or error",
        )),
    }
}

fn parse_notification(value: Value) -> io::Result<Notification> {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || value.get("id").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid TIP notification envelope",
        ));
    }
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| !method.is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "TIP notification has no method")
        })?;
    Ok(Notification {
        method: method.to_owned(),
        params: value.get("params").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;
    use tempfile::tempdir;

    fn listener() -> (tempfile::TempDir, PathBuf, UnixListener) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tip.sock");
        let listener = UnixListener::bind(&path).unwrap();
        (dir, path, listener)
    }

    fn server_read(stream: &mut UnixStream) -> Value {
        read_message(stream).unwrap()
    }

    #[test]
    fn call_frames_request_and_validates_response() {
        let (_dir, path, listener) = listener();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = server_read(&mut stream);
            assert_eq!(request["method"], "hello");
            assert_eq!(request["params"], json!({}));
            write_message(
                &mut stream,
                &json!({"jsonrpc":"2.0", "id":1, "result":{"protocolVersion":3}}),
            )
            .unwrap();
        });

        let mut client = Client::connect_to(path).unwrap();
        let result = client.call("hello", json!({})).unwrap();
        assert_eq!(result["protocolVersion"], 3);
        server.join().unwrap();
    }

    #[test]
    fn rpc_error_becomes_io_error() {
        let (_dir, path, listener) = listener();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = server_read(&mut stream);
            write_message(
                &mut stream,
                &json!({"jsonrpc":"2.0", "id":1, "error":{"code":-32602, "message":"Unknown key"}}),
            )
            .unwrap();
        });

        let error = Client::connect_to(path)
            .unwrap()
            .call("config.get", json!({"key":"missing"}))
            .unwrap_err();
        assert_eq!(error.to_string(), "Unknown key");
        server.join().unwrap();
    }

    #[test]
    fn subscription_receives_notification() {
        let (_dir, path, listener) = listener();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = server_read(&mut stream);
            assert_eq!(request["method"], "events.subscribe");
            write_message(
                &mut stream,
                &json!({"jsonrpc":"2.0", "id":1, "result":{"subscribed":true}}),
            )
            .unwrap();
            write_message(
                &mut stream,
                &json!({"jsonrpc":"2.0", "method":"engine.changed", "params":{"activeKeyboardEngine":"rime"}}),
            )
            .unwrap();
        });

        let client = Client::connect_to(path).unwrap();
        let mut subscription = client.subscribe(&[]).unwrap();
        let event = loop {
            if let Some(event) = subscription.recv().unwrap() {
                break event;
            }
        };
        assert_eq!(event.method, "engine.changed");
        assert_eq!(event.params["activeKeyboardEngine"], "rime");
        server.join().unwrap();
    }

    #[test]
    fn subscription_buffers_a_notification_split_across_reads() {
        let (_dir, path, listener) = listener();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = server_read(&mut stream);
            write_message(
                &mut stream,
                &json!({"jsonrpc":"2.0", "id":1, "result":{"subscribed":true}}),
            )
            .unwrap();

            let payload = serde_json::to_vec(
                &json!({"jsonrpc":"2.0", "method":"config.changed", "params":{}}),
            )
            .unwrap();
            let frame = [&(payload.len() as u32).to_be_bytes()[..], &payload].concat();
            stream.write_all(&frame[..2]).unwrap();
            thread::sleep(Duration::from_millis(20));
            stream.write_all(&frame[2..]).unwrap();
        });

        let mut subscription = Client::connect_to(path).unwrap().subscribe(&[]).unwrap();
        let event = loop {
            if let Some(event) = subscription.recv().unwrap() {
                break event;
            }
        };
        assert_eq!(event.method, "config.changed");
        server.join().unwrap();
    }
}
