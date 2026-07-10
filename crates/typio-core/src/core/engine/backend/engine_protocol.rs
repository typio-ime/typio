//! Typio Engine Protocol framing.
//!
//! The engine protocol channel is a private Unix file descriptor passed to the
//! engine process. Standard output and standard error are reserved for logs.
//! Messages are length-bounded binary frames with a fixed network-order header.

use super::super::{EngineError, Result};
use std::io::{Read, Write};

/// File descriptor number used by engine processes for the protocol.
pub const ENGINE_PROTOCOL_FD: i32 = 3;

/// Magic number carried by every frame: ASCII `TYEP`.
pub const FRAME_MAGIC: u32 = 0x5459_4550;

/// Major protocol version for Typio Engine Protocol.
pub const PROTOCOL_MAJOR: u16 = 1;

/// Minor protocol version for Typio Engine Protocol.
pub const PROTOCOL_MINOR: u16 = 0;

/// Maximum payload accepted from an engine process.
pub const MAX_PAYLOAD_LEN: usize = 8 << 20;

const HEADER_LEN: usize = 28;

/// Engine Protocol message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    /// Engine process hello.
    EngineHello = 1,
    /// Host runtime hello.
    HostHello = 2,
    /// Host request.
    Request = 3,
    /// Engine process response.
    Response = 4,
    /// Engine process event.
    Event = 5,
    /// Protocol or engine error.
    Error = 6,
}

impl MessageType {
    fn from_u32(value: u32) -> Result<Self> {
        match value {
            1 => Ok(Self::EngineHello),
            2 => Ok(Self::HostHello),
            3 => Ok(Self::Request),
            4 => Ok(Self::Response),
            5 => Ok(Self::Event),
            6 => Ok(Self::Error),
            _ => Err(EngineError::Transport(format!(
                "unknown engine-protocol message type {value}"
            ))),
        }
    }
}

/// Decoded engine protocol frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Message type.
    pub message_type: MessageType,
    /// Protocol flags. Reserved for future use.
    pub flags: u32,
    /// Request identifier. Responses must echo their request id.
    pub request_id: u64,
    /// Opaque schema-owned payload bytes.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Build a frame with no flags.
    pub fn new(message_type: MessageType, request_id: u64, payload: Vec<u8>) -> Self {
        Self {
            message_type,
            flags: 0,
            request_id,
            payload,
        }
    }
}

/// Read one frame from a blocking stream.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Frame> {
    let mut header = [0u8; HEADER_LEN];
    reader
        .read_exact(&mut header)
        .map_err(|e| EngineError::Transport(format!("engine-protocol header read failed: {e}")))?;

    let magic = u32::from_be_bytes(header[0..4].try_into().unwrap());
    if magic != FRAME_MAGIC {
        return Err(EngineError::Transport(format!(
            "engine-protocol bad frame magic 0x{magic:08x}"
        )));
    }

    let major = u16::from_be_bytes(header[4..6].try_into().unwrap());
    let minor = u16::from_be_bytes(header[6..8].try_into().unwrap());
    if major != PROTOCOL_MAJOR {
        return Err(EngineError::Transport(format!(
            "engine-protocol incompatible protocol {major}.{minor}"
        )));
    }

    let message_type =
        MessageType::from_u32(u32::from_be_bytes(header[8..12].try_into().unwrap()))?;
    let flags = u32::from_be_bytes(header[12..16].try_into().unwrap());
    let request_id = u64::from_be_bytes(header[16..24].try_into().unwrap());
    let payload_len = u32::from_be_bytes(header[24..28].try_into().unwrap()) as usize;
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(EngineError::Transport(format!(
            "engine-protocol oversized payload {payload_len}"
        )));
    }

    let mut payload = vec![0u8; payload_len];
    reader
        .read_exact(&mut payload)
        .map_err(|e| EngineError::Transport(format!("engine-protocol payload read failed: {e}")))?;

    Ok(Frame {
        message_type,
        flags,
        request_id,
        payload,
    })
}

/// Write one frame to a blocking stream.
pub fn write_frame<W: Write>(writer: &mut W, frame: &Frame) -> Result<()> {
    if frame.payload.len() > MAX_PAYLOAD_LEN {
        return Err(EngineError::Transport(format!(
            "engine-protocol oversized payload {}",
            frame.payload.len()
        )));
    }

    let mut header = [0u8; HEADER_LEN];
    header[0..4].copy_from_slice(&FRAME_MAGIC.to_be_bytes());
    header[4..6].copy_from_slice(&PROTOCOL_MAJOR.to_be_bytes());
    header[6..8].copy_from_slice(&PROTOCOL_MINOR.to_be_bytes());
    header[8..12].copy_from_slice(&(frame.message_type as u32).to_be_bytes());
    header[12..16].copy_from_slice(&frame.flags.to_be_bytes());
    header[16..24].copy_from_slice(&frame.request_id.to_be_bytes());
    header[24..28].copy_from_slice(&(frame.payload.len() as u32).to_be_bytes());

    writer
        .write_all(&header)
        .and_then(|_| writer.write_all(&frame.payload))
        .and_then(|_| writer.flush())
        .map_err(|e| EngineError::Transport(format!("engine-protocol frame write failed: {e}")))
}
