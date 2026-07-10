//! TIP v1 client (typio ADR-0008) — UDS + length-prefixed JSON-RPC 2.0.

use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

const TIP_MAX_FRAME: usize = 1 << 20; /* 1 MiB */
const TIP_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Canonical UDS socket path.
///
/// Prefers `$XDG_RUNTIME_DIR/typio/daemon.sock`, falls back to
/// `~/.local/share/typio/daemon.sock`, then `/tmp/typio-daemon.sock`.
pub fn socket_path() -> Option<PathBuf> {
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR")
        && !runtime_dir.is_empty()
    {
        let mut p = PathBuf::from(runtime_dir);
        p.push("typio");
        p.push("daemon.sock");
        return Some(p);
    }
    if let Ok(home) = env::var("HOME")
        && !home.is_empty()
    {
        let mut p = PathBuf::from(home);
        p.push(".local");
        p.push("share");
        p.push("typio");
        p.push("daemon.sock");
        return Some(p);
    }
    Some(PathBuf::from("/tmp/typio-daemon.sock"))
}

/// JSON-RPC 2.0 client over UDS with 4-byte BE length prefix (TIP v1).
pub struct IpcClient {
    stream: UnixStream,
    next_id: i64,
}

impl IpcClient {
    pub fn connect() -> io::Result<Self> {
        let path = socket_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not resolve daemon socket path",
            )
        })?;
        let stream = UnixStream::connect(&path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{e}\n\nIs typio running? Start `typio.service` or run `typio --verbose`."),
            )
        })?;
        stream.set_read_timeout(Some(TIP_IO_TIMEOUT))?;
        stream.set_write_timeout(Some(TIP_IO_TIMEOUT))?;
        Ok(IpcClient { stream, next_id: 1 })
    }

    /// Send a JSON-RPC request and wait for the response.
    pub fn call(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("request id exhausted"))?;

        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let bytes = serde_json::to_vec(&req)?;
        if bytes.len() > TIP_MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "request too large",
            ));
        }
        let len_be = (bytes.len() as u32).to_be_bytes();
        self.stream.write_all(&len_be)?;
        self.stream.write_all(&bytes)?;
        self.stream.flush()?;

        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;
        if resp_len > TIP_MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response too large",
            ));
        }
        let mut buf = vec![0u8; resp_len];
        self.stream.read_exact(&mut buf)?;
        let resp: Value = serde_json::from_slice(&buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("invalid JSON: {e}"))
        })?;

        if resp.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid JSON-RPC version",
            ));
        }
        if resp.get("id").and_then(Value::as_i64) != Some(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response id mismatch",
            ));
        }
        let result = resp.get("result");
        let error = resp.get("error");
        if result.is_some() == error.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response must contain exactly one of result or error",
            ));
        }
        if let Some(err) = error {
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(io::Error::other(msg.to_string()));
        }
        Ok(result.cloned().unwrap_or(Value::Null))
    }
}
