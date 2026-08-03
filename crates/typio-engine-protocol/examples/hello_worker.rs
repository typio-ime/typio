//! Minimal keyboard worker that speaks Typio Engine Protocol on fd 3.

use std::fs::File;
use std::os::fd::FromRawFd;

use typio_engine_protocol::{
    Availability, ENGINE_PROTOCOL_FD, EngineHello, EngineKind, Frame, HostHello, KeyResult,
    MessageType, Reply, ReplyRecord, Request, read_frame, write_frame,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: The host maps the worker's private protocol socket to fd 3 before
    // exec. This process takes sole ownership of that inherited descriptor.
    let mut channel = unsafe { File::from_raw_fd(ENGINE_PROTOCOL_FD) };

    let engine_hello = EngineHello {
        engine: "hello".to_string(),
        kind: EngineKind::Keyboard,
        schema: Vec::new(),
    };
    write_frame(
        &mut channel,
        &Frame::new(MessageType::EngineHello, 0, engine_hello.encode()),
    )?;

    let host_frame = match read_frame(&mut channel) {
        Ok(frame) => frame,
        // Discovery probes intentionally close after reading EngineHello.
        Err(typio_engine_protocol::ProtocolError::Io(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if host_frame.message_type != MessageType::HostHello || host_frame.request_id != 0 {
        return Err("expected HOST_HELLO with request id 0".into());
    }
    let host_hello = HostHello::decode(&host_frame.payload)?;
    if host_hello.engine != "hello" || host_hello.kind != EngineKind::Keyboard {
        return Err("host identity does not match this worker".into());
    }
    // Real engines use host_hello.config_dir/data_dir/state_dir instead of
    // guessing the daemon's command-line overrides from ambient XDG state.

    loop {
        let frame = read_frame(&mut channel)?;
        if frame.message_type != MessageType::Request || frame.request_id == 0 {
            return Err("expected REQUEST with a nonzero request id".into());
        }
        let request = Request::decode(&frame.payload)?;
        if request == Request::Shutdown {
            return Ok(());
        }

        let records = match request {
            Request::Availability => vec![ReplyRecord::Availability(Availability::Ready)],
            Request::ProcessKey(_) => vec![ReplyRecord::KeyResult(KeyResult::NotHandled)],
            Request::ListModes | Request::ListCommands | Request::GetActiveMode(_) => Vec::new(),
            Request::ProcessAudio(_) => vec![ReplyRecord::Error("NOT_SUPPORTED".to_string())],
            Request::Initialize
            | Request::Deactivate
            | Request::FocusIn(_)
            | Request::FocusOut(_)
            | Request::Reset(_)
            | Request::ReloadConfig
            | Request::SetActiveMode { .. }
            | Request::CommitCandidate { .. }
            | Request::InvokeCommand(_) => vec![ReplyRecord::Ok],
            Request::Shutdown => unreachable!(),
        };
        write_frame(
            &mut channel,
            &Frame::new(
                MessageType::Response,
                frame.request_id,
                Reply { records }.encode(),
            ),
        )?;
    }
}
