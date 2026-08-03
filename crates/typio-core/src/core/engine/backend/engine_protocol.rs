//! Typio Engine Protocol integration.
//!
//! The wire contract lives in the standalone `typio-engine-protocol` crate.
//! This module only maps transport errors into the framework error domain.

use super::super::{EngineError, Result};
use std::io::{Read, Write};

pub use typio_engine_protocol::{
    Availability, Candidate, Command, Composition, ENGINE_PROTOCOL_FD, EngineHello, EngineKind,
    FRAME_MAGIC, Frame, HostHello, KeyEvent, KeyResult, KeyState, MAX_PAYLOAD_LEN, MessageType,
    Mode, ModeSalience, PROTOCOL_MAJOR, PROTOCOL_MINOR, PreeditFormat, PreeditSegment, Reply,
    ReplyRecord, Request, SchemaDefault, SchemaField, SchemaType, decode_hex, decode_hex_string,
    encode_hex, encode_hex_string,
};

/// Read one frame and map a wire error into the framework error domain.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Frame> {
    typio_engine_protocol::read_frame(reader)
        .map_err(|error| EngineError::Transport(format!("engine-protocol read failed: {error}")))
}

/// Write one frame and map a wire error into the framework error domain.
pub fn write_frame<W: Write>(writer: &mut W, frame: &Frame) -> Result<()> {
    typio_engine_protocol::write_frame(writer, frame)
        .map_err(|error| EngineError::Transport(format!("engine-protocol write failed: {error}")))
}
