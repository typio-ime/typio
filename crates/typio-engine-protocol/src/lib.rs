//! Typed wire contract for isolated Typio engine processes.
//!
//! This crate owns the only contract shared by the daemon and engine
//! executables. It contains no host runtime, engine implementation ABI, or
//! platform integration.

mod codec;
mod frame;
mod message;

pub use codec::{decode_hex, decode_hex_string, encode_hex, encode_hex_string};
pub use frame::{
    ENGINE_PROTOCOL_FD, FRAME_MAGIC, Frame, MAX_PAYLOAD_LEN, MessageType, PROTOCOL_MAJOR,
    PROTOCOL_MINOR, read_frame, write_frame,
};
pub use message::{
    Availability, Candidate, Command, Composition, EngineHello, EngineKind, HostHello, KeyEvent,
    KeyResult, KeyState, Mode, ModeSalience, PreeditFormat, PreeditSegment, Reply, ReplyRecord,
    Request, SchemaDefault, SchemaField, SchemaType,
};

use std::fmt;

/// Error returned for malformed frames or typed payloads.
#[derive(Debug)]
pub enum ProtocolError {
    /// The transport could not read or write a complete frame.
    Io(std::io::Error),
    /// A frame or payload violated the protocol contract.
    Invalid(String),
}

impl ProtocolError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

impl From<std::io::Error> for ProtocolError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Result alias for protocol operations.
pub type Result<T> = std::result::Result<T, ProtocolError>;
