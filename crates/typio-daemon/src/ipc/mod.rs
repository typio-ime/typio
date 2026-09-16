//! Typio IPC Protocol (TIP) v3 — the daemon's UDS control surface.
//!
//! JSON-RPC 2.0 over a Unix-domain socket, with `serde_json` for
//! round-tripping and `serde` derives for the envelope.
//!
//! ## Scope
//!
//! This module covers:
//!
//! - **Protocol constants** (method names, topic names, version). See
//!   [`protocol`].
//! - **JSON-RPC 2.0 envelope** (Request, Response, Notification, Error,
//!   Id). See [`framing`].
//! - **Socket path resolution** (`$XDG_RUNTIME_DIR/typio/daemon.sock`
//!   first, `~/.local/share/typio/daemon.sock` fallback,
//!   `/tmp/typio-daemon.sock` last resort). See [`protocol::socket_path`].
//!
//! Per-method typed request/response structs (e.g. `HelloParams`,
//! `ConfigGetResult`) are deliberately not modelled: the daemon dispatches
//! per-method over `params`/`result` JSON values, so the envelope stays the
//! only strongly-typed layer. See [`crate::service`] and [`crate::ipc_bus`]
//! for the handlers.
//!
//! ## Layout
//!
//! - [`protocol`] — constants + socket path
//! - [`framing`] — JSON-RPC 2.0 envelope types

pub mod framing;
pub mod protocol;
