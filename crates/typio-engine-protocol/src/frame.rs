use crate::{ProtocolError, Result};
use std::io::{Read, Write};

/// File descriptor reserved for the private engine channel.
pub const ENGINE_PROTOCOL_FD: i32 = 3;

/// ASCII `TYEP`, carried by every frame.
pub const FRAME_MAGIC: u32 = 0x5459_4550;

/// Current protocol major version.
pub const PROTOCOL_MAJOR: u16 = 1;

/// Current protocol minor version.
pub const PROTOCOL_MINOR: u16 = 0;

/// Maximum payload accepted from either peer.
pub const MAX_PAYLOAD_LEN: usize = 8 << 20;

const HEADER_LEN: usize = 28;

/// Engine Protocol frame kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    /// Initial metadata sent by an engine process.
    EngineHello = 1,
    /// Handshake acknowledgement sent by the host.
    HostHello = 2,
    /// Host operation request.
    Request = 3,
    /// Engine operation response.
    Response = 4,
    /// Unsolicited engine event.
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
            _ => Err(ProtocolError::invalid(format!(
                "unknown engine-protocol message type {value}"
            ))),
        }
    }
}

/// One decoded Engine Protocol frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Frame kind.
    pub message_type: MessageType,
    /// Reserved protocol flags.
    pub flags: u32,
    /// Request identifier echoed by responses.
    pub request_id: u64,
    /// Schema-owned payload bytes.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Construct a frame with no flags.
    pub fn new(message_type: MessageType, request_id: u64, payload: Vec<u8>) -> Self {
        Self {
            message_type,
            flags: 0,
            request_id,
            payload,
        }
    }
}

/// Read and validate one complete frame.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Frame> {
    let mut header = [0_u8; HEADER_LEN];
    reader.read_exact(&mut header).map_err(ProtocolError::Io)?;

    let magic = u32::from_be_bytes(header[0..4].try_into().unwrap());
    if magic != FRAME_MAGIC {
        return Err(ProtocolError::invalid(format!(
            "engine-protocol bad frame magic 0x{magic:08x}"
        )));
    }

    let major = u16::from_be_bytes(header[4..6].try_into().unwrap());
    let minor = u16::from_be_bytes(header[6..8].try_into().unwrap());
    if major != PROTOCOL_MAJOR {
        return Err(ProtocolError::invalid(format!(
            "engine-protocol incompatible version {major}.{minor}"
        )));
    }

    let message_type =
        MessageType::from_u32(u32::from_be_bytes(header[8..12].try_into().unwrap()))?;
    let flags = u32::from_be_bytes(header[12..16].try_into().unwrap());
    let request_id = u64::from_be_bytes(header[16..24].try_into().unwrap());
    let payload_len = u32::from_be_bytes(header[24..28].try_into().unwrap()) as usize;
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(ProtocolError::invalid(format!(
            "engine-protocol oversized payload {payload_len}"
        )));
    }

    let mut payload = vec![0_u8; payload_len];
    reader.read_exact(&mut payload).map_err(ProtocolError::Io)?;
    Ok(Frame {
        message_type,
        flags,
        request_id,
        payload,
    })
}

/// Validate and write one complete frame.
pub fn write_frame<W: Write>(writer: &mut W, frame: &Frame) -> Result<()> {
    if frame.payload.len() > MAX_PAYLOAD_LEN {
        return Err(ProtocolError::invalid(format!(
            "engine-protocol oversized payload {}",
            frame.payload.len()
        )));
    }

    let mut header = [0_u8; HEADER_LEN];
    header[0..4].copy_from_slice(&FRAME_MAGIC.to_be_bytes());
    header[4..6].copy_from_slice(&PROTOCOL_MAJOR.to_be_bytes());
    header[6..8].copy_from_slice(&PROTOCOL_MINOR.to_be_bytes());
    header[8..12].copy_from_slice(&(frame.message_type as u32).to_be_bytes());
    header[12..16].copy_from_slice(&frame.flags.to_be_bytes());
    header[16..24].copy_from_slice(&frame.request_id.to_be_bytes());
    header[24..28].copy_from_slice(&(frame.payload.len() as u32).to_be_bytes());

    writer.write_all(&header).map_err(ProtocolError::Io)?;
    writer
        .write_all(&frame.payload)
        .map_err(ProtocolError::Io)?;
    writer.flush().map_err(ProtocolError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_round_trip() {
        let expected = Frame {
            message_type: MessageType::Response,
            flags: 0x10,
            request_id: 42,
            payload: b"RESULT\tHANDLED\n".to_vec(),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &expected).unwrap();
        assert_eq!(read_frame(&mut Cursor::new(bytes)).unwrap(), expected);
    }

    #[test]
    fn rejects_bad_magic_version_type_and_size() {
        let mut header = [0_u8; HEADER_LEN];
        header[0..4].copy_from_slice(&FRAME_MAGIC.to_be_bytes());
        header[4..6].copy_from_slice(&PROTOCOL_MAJOR.to_be_bytes());
        header[8..12].copy_from_slice(&(MessageType::Request as u32).to_be_bytes());

        let mut bad_magic = header;
        bad_magic[0] = 0;
        assert!(read_frame(&mut Cursor::new(bad_magic)).is_err());

        let mut bad_version = header;
        bad_version[4..6].copy_from_slice(&(PROTOCOL_MAJOR + 1).to_be_bytes());
        assert!(read_frame(&mut Cursor::new(bad_version)).is_err());

        let mut bad_type = header;
        bad_type[8..12].copy_from_slice(&99_u32.to_be_bytes());
        assert!(read_frame(&mut Cursor::new(bad_type)).is_err());

        let mut oversized = header;
        oversized[24..28].copy_from_slice(&((MAX_PAYLOAD_LEN + 1) as u32).to_be_bytes());
        assert!(read_frame(&mut Cursor::new(oversized)).is_err());
    }
}
