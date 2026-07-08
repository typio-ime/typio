//! Out-of-process engine backend.
//!
//! Typio Engine Protocol runs over a private Unix file descriptor passed to the
//! engine process as fd 3. Standard output and standard error are reserved for logs.
//! Each request and response is carried in a bounded binary frame.
//!
//! A reply may also carry an `ACTIVE_MODE` line reflecting the engine's
//! current keyboard mode after the request was applied. Keyboard engines are
//! host-driven — their internal mode only changes as a side effect of a
//! request — so the reply is the canonical, race-free carrier for mode
//! changes. The backend caches the latest reported mode and surfaces a
//! transition through [`KeyboardEngine::take_changed_mode`], which the
//! framework turns into a host notification (no polling, no async channel).

use super::super::{
    Command, Engine, EngineAvailability, EngineError, EngineInfo, EngineMode, InputContext,
    InstanceHandle, KeyEvent, KeyProcessResult, KeyState, KeyboardEngine, ModeSalience, Result,
    VoiceEngine,
};
use super::engine_protocol::{ENGINE_PROTOCOL_FD, Frame, MessageType, read_frame, write_frame};
use std::ffi::CString;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::Mutex;
use std::time::Duration;

const ENGINE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const ENGINE_REQUEST_TIMEOUT: Duration = Duration::from_millis(100);

/// Out-of-process backend.
#[derive(Debug)]
pub struct ProcessBackend {
    info: EngineInfo,
    argv: Vec<String>,
    engine: Option<ProcessEngine>,
}

impl ProcessBackend {
    /// Construct a backend from an executable argv vector.
    pub fn new(info: EngineInfo, argv: Vec<String>) -> Self {
        Self {
            info,
            argv,
            engine: None,
        }
    }

    /// Immutable engine metadata.
    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// Mutable engine metadata, for registry-side enrichment (languages).
    pub(crate) fn info_mut(&mut self) -> &mut EngineInfo {
        &mut self.info
    }

    /// Start the engine process if needed.
    pub fn instantiate(&mut self) -> Result<()> {
        if self.engine.is_some() {
            return Ok(());
        }
        if self.argv.is_empty() || self.argv[0].is_empty() {
            return Err(EngineError::InvalidArgument);
        }
        self.engine = Some(ProcessEngine::spawn(self.info.clone(), &self.argv)?);
        Ok(())
    }

    /// Whether the engine process has been started.
    pub fn is_instantiated(&self) -> bool {
        self.engine.is_some()
    }

    /// Execute a closure with a mutable reference to the engine process.
    pub fn with_engine<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn Engine) -> R,
    {
        self.engine.as_mut().map(|e| f(e))
    }

    /// Execute a closure with an immutable reference to the engine process.
    pub fn with_engine_ref<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&dyn Engine) -> R,
    {
        self.engine.as_ref().map(|e| f(e))
    }

    /// Stop the engine process.
    pub fn destroy(&mut self) {
        self.engine.take();
    }
}

struct ProcessEngine {
    info: EngineInfo,
    process: Mutex<EngineProcess>,
    /// Latest active mode observed in an engine response, plus whether it has
    /// changed since [`take_changed_mode`](ProcessEngine::take_changed_mode) last
    /// drained it. Behind its own lock so observation never blocks on the
    /// process I/O lock.
    observed: Mutex<ModeObservation>,
}

#[derive(Default)]
struct ModeObservation {
    last: Option<EngineMode>,
    changed: bool,
}

struct EngineProcess {
    child: Child,
    stream: UnixStream,
    next_request_id: u64,
}

impl std::fmt::Debug for ProcessEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessEngine")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl ProcessEngine {
    fn spawn(info: EngineInfo, argv: &[String]) -> Result<Self> {
        let (mut host_stream, engine_stream) = UnixStream::pair().map_err(|e| {
            EngineError::Transport(format!("engine-protocol socketpair failed: {e}"))
        })?;
        host_stream
            .set_read_timeout(Some(ENGINE_HANDSHAKE_TIMEOUT))
            .map_err(|e| {
                EngineError::Transport(format!("engine-protocol timeout setup failed: {e}"))
            })?;
        host_stream
            .set_write_timeout(Some(ENGINE_HANDSHAKE_TIMEOUT))
            .map_err(|e| {
                EngineError::Transport(format!("engine-protocol timeout setup failed: {e}"))
            })?;

        let mut command = ProcessCommand::new(&argv[0]);
        let engine_fd = engine_stream.as_raw_fd();
        command.args(&argv[1..]);
        command
            .env("TYPIO_ENGINE_PROTOCOL", "1.0")
            .env("TYPIO_ENGINE_FD", ENGINE_PROTOCOL_FD.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(engine_fd, ENGINE_PROTOCOL_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if engine_fd != ENGINE_PROTOCOL_FD {
                    libc::close(engine_fd);
                }
                Ok(())
            });
        }

        // ETXTBSY guard: an engine binary that was just written and marked
        // executable — or one being replaced by a package upgrade — can still be
        // open for writing when we execve it, which fails with ETXTBSY. The
        // condition is transient, so retry a few times with a short backoff
        // before surfacing it. Any other spawn error fails fast.
        const SPAWN_RETRIES: u32 = 10;
        let child = {
            let mut attempt = 0;
            loop {
                match command.spawn() {
                    Ok(child) => break child,
                    Err(e)
                        if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < SPAWN_RETRIES =>
                    {
                        attempt += 1;
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(e) => {
                        return Err(EngineError::Transport(format!("spawn {}: {e}", argv[0])));
                    }
                }
            }
        };
        drop(engine_stream);

        let hello = read_frame(&mut host_stream)?;
        if hello.message_type != MessageType::EngineHello {
            return Err(EngineError::Transport(format!(
                "engine-protocol expected hello, got {:?}",
                hello.message_type
            )));
        }
        let hello_text = String::from_utf8(hello.payload).map_err(|e| {
            EngineError::Transport(format!("engine-protocol invalid hello utf-8: {e}"))
        })?;
        validate_engine_hello(&info, &hello_text)?;

        let host_hello = format!(
            "protocol\t1.0\nengine\t{}\ntype\t{}",
            info.name,
            engine_type_name(info.engine_type)
        );
        write_frame(
            &mut host_stream,
            &Frame::new(MessageType::HostHello, 0, host_hello.into_bytes()),
        )?;
        host_stream
            .set_read_timeout(Some(ENGINE_REQUEST_TIMEOUT))
            .map_err(|e| {
                EngineError::Transport(format!("engine-protocol timeout setup failed: {e}"))
            })?;
        host_stream
            .set_write_timeout(Some(ENGINE_REQUEST_TIMEOUT))
            .map_err(|e| {
                EngineError::Transport(format!("engine-protocol timeout setup failed: {e}"))
            })?;

        Ok(Self {
            info,
            process: Mutex::new(EngineProcess {
                child,
                stream: host_stream,
                next_request_id: 1,
            }),
            observed: Mutex::new(ModeObservation::default()),
        })
    }

    /// Fold an `ACTIVE_MODE` line from a reply into the observation cache,
    /// flagging a change when the active mode's identity differs from the
    /// previously observed mode.
    ///
    /// Identity is the mode `id` (ADR-0011), the same predicate the host-side
    /// `engine_mode_equal` uses to de-duplicate. The two layers must agree:
    /// surfacing a "change" here that the host then collapses as identical
    /// (or vice versa) would desync the indicator/tray from the cached mode.
    /// Every other field (badge, icon, salience) is a function of the id, so
    /// the id alone is the canonical change signal.
    fn observe_active_mode(&self, mode: &EngineMode) {
        if let Ok(mut observed) = self.observed.lock() {
            let differs = observed
                .last
                .as_ref()
                .map(|prev| prev.id != mode.id)
                .unwrap_or(true);
            if differs {
                observed.last = Some(mode.clone());
                observed.changed = true;
            }
        }
    }

    fn request(&self, line: &str, ctx: Option<&mut InputContext>) -> Result<WorkerReply> {
        let mut process = self
            .process
            .lock()
            .map_err(|_| EngineError::Transport("worker lock poisoned".into()))?;

        let request_id = process.next_request_id;
        process.next_request_id = process.next_request_id.wrapping_add(1).max(1);
        write_frame(
            &mut process.stream,
            &Frame::new(MessageType::Request, request_id, line.as_bytes().to_vec()),
        )?;

        let frame = read_frame(&mut process.stream)?;
        if frame.request_id != request_id {
            return Err(EngineError::Transport(format!(
                "engine-protocol response id mismatch: expected {request_id}, got {}",
                frame.request_id
            )));
        }
        if frame.message_type == MessageType::Error {
            let message = String::from_utf8_lossy(&frame.payload);
            return Err(EngineError::Transport(message.into_owned()));
        }
        if frame.message_type != MessageType::Response {
            return Err(EngineError::Transport(format!(
                "engine-protocol expected response, got {:?}",
                frame.message_type
            )));
        }

        let mut reply = WorkerReply::default();
        let payload = String::from_utf8(frame.payload).map_err(|e| {
            EngineError::Transport(format!("engine-protocol invalid response utf-8: {e}"))
        })?;
        for response in payload.lines() {
            let response = response.trim_end_matches(['\r', '\n']);
            if response == "END" || response.is_empty() {
                continue;
            }
            self.handle_response_line(response, &mut reply, ctx.as_deref())?;
        }
        if let Some(err) = reply.error.take() {
            return Err(EngineError::Transport(err));
        }
        if let Some(mode) = reply.active_mode.as_ref() {
            self.observe_active_mode(mode);
        }
        Ok(reply)
    }

    fn handle_response_line(
        &self,
        line: &str,
        reply: &mut WorkerReply,
        ctx: Option<&InputContext>,
    ) -> Result<()> {
        let mut parts = line.splitn(2, '\t');
        let op = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("");
        match op {
            "OK" => {}
            "ERR" => reply.error = Some(arg.to_string()),
            "RESULT" => reply.key_result = parse_key_result(arg),
            "AVAILABILITY" => reply.availability = parse_availability(arg),
            "TEXT" => reply.text = Some(decode_hex_to_string(arg)?),
            "MODE" => reply.modes.push(parse_mode(arg)?),
            "ACTIVE_MODE" => reply.active_mode = Some(parse_mode(arg)?),
            "COMPOSITION" => {
                if let Some(ctx) = ctx {
                    apply_composition(ctx, arg)?;
                }
            }
            "COMMIT" => {
                if let Some(ctx) = ctx {
                    let text = decode_hex_to_string(arg)?;
                    let c_text = CString::new(text).map_err(|_| EngineError::InvalidArgument)?;
                    crate::input_context::typio_input_context_commit(
                        ctx.as_raw() as *mut crate::input_context::TypioInputContext,
                        c_text.as_ptr(),
                    );
                }
            }
            "CLEAR" => {
                if let Some(ctx) = ctx {
                    crate::input_context::typio_input_context_clear(
                        ctx.as_raw() as *mut crate::input_context::TypioInputContext
                    );
                }
            }
            "" => {}
            other => {
                return Err(EngineError::Transport(format!(
                    "unknown engine response op '{other}'"
                )));
            }
        }
        Ok(())
    }
}

impl Drop for ProcessEngine {
    fn drop(&mut self) {
        if let Ok(mut process) = self.process.lock() {
            let request_id = process.next_request_id;
            let _ = write_frame(
                &mut process.stream,
                &Frame::new(MessageType::Request, request_id, b"shutdown".to_vec()),
            );
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }
}

impl Engine for ProcessEngine {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn init(&mut self, _instance: &mut InstanceHandle) -> Result<()> {
        self.request("init", None).map(|_| ())
    }

    fn deactivate(&mut self) {
        let _ = self.request("deactivate", None);
    }

    fn focus_in(&mut self, ctx: &mut InputContext) {
        let line = format!("focus-in\t{}", context_id(ctx));
        let _ = self.request(&line, Some(ctx));
    }

    fn focus_out(&mut self, ctx: &mut InputContext) {
        let line = format!("focus-out\t{}", context_id(ctx));
        let _ = self.request(&line, Some(ctx));
    }

    fn reset(&mut self, ctx: &mut InputContext) {
        let line = format!("reset\t{}", context_id(ctx));
        let _ = self.request(&line, Some(ctx));
    }

    fn reload_config(&mut self) -> Result<()> {
        self.request("reload-config", None).map(|_| ())
    }

    fn as_keyboard(&mut self) -> Option<&mut dyn KeyboardEngine> {
        if self.info.engine_type == super::super::EngineType::Keyboard {
            Some(self)
        } else {
            None
        }
    }

    fn as_keyboard_ref(&self) -> Option<&dyn KeyboardEngine> {
        if self.info.engine_type == super::super::EngineType::Keyboard {
            Some(self)
        } else {
            None
        }
    }

    fn as_voice(&mut self) -> Option<&mut dyn VoiceEngine> {
        if self.info.engine_type == super::super::EngineType::Voice {
            Some(self)
        } else {
            None
        }
    }

    fn as_voice_ref(&self) -> Option<&dyn VoiceEngine> {
        if self.info.engine_type == super::super::EngineType::Voice {
            Some(self)
        } else {
            None
        }
    }

    fn list_commands(&self) -> Vec<Command> {
        vec![]
    }

    fn availability(&self) -> EngineAvailability {
        self.request("availability", None)
            .ok()
            .and_then(|reply| reply.availability)
            .unwrap_or(EngineAvailability::Failed)
    }
}

impl KeyboardEngine for ProcessEngine {
    fn process_key(&mut self, ctx: &mut InputContext, event: &KeyEvent) -> KeyProcessResult {
        let state = match event.state {
            KeyState::Press => "press",
            KeyState::Release => "release",
        };
        let line = format!(
            "process-key\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            context_id(ctx),
            state,
            event.code,
            keysym_to_u32(event.sym),
            event.modifiers,
            event.unicode,
            event.time,
            u8::from(event.is_repeat),
            event.base_keysym
        );
        self.request(&line, Some(ctx))
            .map(|reply| reply.key_result.unwrap_or(KeyProcessResult::NotHandled))
            .unwrap_or(KeyProcessResult::NotHandled)
    }

    fn list_modes(&self) -> Vec<EngineMode> {
        self.request("list-modes", None)
            .map(|reply| reply.modes)
            .unwrap_or_default()
    }

    fn get_active_mode(&self, ctx: &InputContext) -> Option<EngineMode> {
        let line = format!("get-active-mode\t{}", context_id(ctx));
        self.request(&line, None)
            .ok()
            .and_then(|reply| reply.active_mode)
    }

    fn set_active_mode(&mut self, ctx: &mut InputContext, mode_id: Option<&str>) -> Result<()> {
        let encoded = mode_id.map(hex_encode_str).unwrap_or_default();
        let line = format!("set-active-mode\t{}\t{}", context_id(ctx), encoded);
        self.request(&line, Some(ctx)).map(|_| ())
    }

    fn commit_candidate(&mut self, ctx: &mut InputContext, candidate_index: i32) -> Result<()> {
        let line = format!("commit-candidate\t{}\t{}", context_id(ctx), candidate_index);
        self.request(&line, Some(ctx)).map(|_| ())
    }

    fn take_changed_mode(&mut self) -> Option<EngineMode> {
        let mut observed = self.observed.lock().ok()?;
        if observed.changed {
            observed.changed = false;
            observed.last.clone()
        } else {
            None
        }
    }
}

impl VoiceEngine for ProcessEngine {
    fn process_audio(&self, samples: &[f32]) -> Option<String> {
        let mut bytes = Vec::with_capacity(samples.len() * std::mem::size_of::<f32>());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let line = format!("process-audio\t{}", hex_encode_bytes(&bytes));
        self.request(&line, None).ok().and_then(|reply| reply.text)
    }
}

#[derive(Default)]
struct WorkerReply {
    error: Option<String>,
    key_result: Option<KeyProcessResult>,
    availability: Option<EngineAvailability>,
    text: Option<String>,
    modes: Vec<EngineMode>,
    active_mode: Option<EngineMode>,
}

fn context_id(ctx: &InputContext) -> usize {
    ctx.as_raw() as usize
}

fn parse_key_result(value: &str) -> Option<KeyProcessResult> {
    match value {
        "NOT_HANDLED" => Some(KeyProcessResult::NotHandled),
        "HANDLED" => Some(KeyProcessResult::Handled),
        "COMPOSING" => Some(KeyProcessResult::Handled),
        "COMMITTED" => Some(KeyProcessResult::Handled),
        "PASS_THROUGH" => Some(KeyProcessResult::PassThrough),
        _ => None,
    }
}

fn parse_availability(value: &str) -> Option<EngineAvailability> {
    match value {
        "UNINITIALIZED" => Some(EngineAvailability::Uninitialized),
        "PREPARING" => Some(EngineAvailability::Preparing),
        "READY" => Some(EngineAvailability::Ready),
        "FAILED" => Some(EngineAvailability::Failed),
        _ => None,
    }
}

fn engine_type_name(engine_type: super::super::EngineType) -> &'static str {
    match engine_type {
        super::super::EngineType::Keyboard => "keyboard",
        super::super::EngineType::Voice => "voice",
    }
}

fn validate_engine_hello(info: &EngineInfo, payload: &str) -> Result<()> {
    let mut protocol_ok = false;
    let mut worker_engine = None;
    let mut worker_type = None;

    for line in payload.lines() {
        let mut fields = line.splitn(2, '\t');
        let key = fields.next().unwrap_or("");
        let value = fields.next().unwrap_or("");
        match key {
            "protocol" => protocol_ok = value == "1.0",
            "engine" => worker_engine = Some(value),
            "type" => worker_type = Some(value),
            _ => {}
        }
    }

    if !protocol_ok {
        return Err(EngineError::Transport(
            "engine-protocol hello missing compatible protocol".into(),
        ));
    }
    if worker_engine != Some(info.name.as_str()) {
        return Err(EngineError::Transport(format!(
            "engine-protocol engine mismatch: manifest '{}' engine '{}'",
            info.name,
            worker_engine.unwrap_or("")
        )));
    }
    let expected_type = engine_type_name(info.engine_type);
    if worker_type != Some(expected_type) {
        return Err(EngineError::Transport(format!(
            "engine-protocol type mismatch: manifest '{}' engine '{}'",
            expected_type,
            worker_type.unwrap_or("")
        )));
    }

    Ok(())
}

fn keysym_to_u32(sym: super::super::KeySym) -> u32 {
    match sym {
        super::super::KeySym::Ascii(c) => c as u32,
        super::super::KeySym::ShiftL => 0xffe1,
        super::super::KeySym::ShiftR => 0xffe2,
        super::super::KeySym::ControlL => 0xffe3,
        super::super::KeySym::ControlR => 0xffe4,
        super::super::KeySym::AltL => 0xffe9,
        super::super::KeySym::AltR => 0xffea,
        super::super::KeySym::MetaL => 0xffeb,
        super::super::KeySym::MetaR => 0xffec,
        super::super::KeySym::BackSpace => 0xff08,
        super::super::KeySym::Tab => 0xff09,
        super::super::KeySym::Return => 0xff0d,
        super::super::KeySym::Escape => 0xff1b,
        super::super::KeySym::Left => 0xff51,
        super::super::KeySym::Up => 0xff52,
        super::super::KeySym::Right => 0xff53,
        super::super::KeySym::Down => 0xff54,
        super::super::KeySym::Home => 0xff50,
        super::super::KeySym::End => 0xff57,
        super::super::KeySym::PageUp => 0xff55,
        super::super::KeySym::PageDown => 0xff56,
        super::super::KeySym::F(n) => 0xffbd + u32::from(n),
        super::super::KeySym::Raw(v) => v,
    }
}

fn decode_hex_to_string(value: &str) -> Result<String> {
    let bytes = decode_hex(value)?;
    String::from_utf8(bytes).map_err(|e| EngineError::Transport(format!("invalid utf-8: {e}")))
}

fn hex_encode_str(value: &str) -> String {
    hex_encode_bytes(value.as_bytes())
}

fn hex_encode_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn apply_composition(ctx: &InputContext, payload: &str) -> Result<()> {
    let fields: Vec<&str> = payload.split('\t').collect();
    if fields.len() < 10 {
        return Err(EngineError::Transport("short composition payload".into()));
    }

    let cursor_pos = parse_i32(fields[0]);
    let page = parse_i32(fields[1]);
    let page_size = parse_i32(fields[2]);
    let total = parse_i32(fields[3]);
    let selected = parse_i32(fields[4]);
    let has_prev = parse_bool(fields[5]);
    let has_next = parse_bool(fields[6]);
    let host_managed_selection = parse_u32(fields[7]);
    let segment_values = decode_hex_list(fields[8])?;
    let candidate_values = decode_hex_list(fields[9])?;

    let segment_cstrings: Vec<CString> = segment_values
        .into_iter()
        .map(|s| CString::new(s).unwrap_or_default())
        .collect();
    let candidate_cstrings: Vec<CString> = candidate_values
        .into_iter()
        .map(|s| CString::new(s).unwrap_or_default())
        .collect();

    let segments: Vec<typio_abi::TypioPreeditSegment> = segment_cstrings
        .iter()
        .map(|s| typio_abi::TypioPreeditSegment {
            text: s.as_ptr(),
            format: typio_abi::TypioPreeditFormat::TypioPreeditUnderline as u32,
        })
        .collect();
    let candidates: Vec<typio_abi::TypioCandidate> = candidate_cstrings
        .iter()
        .map(|s| typio_abi::TypioCandidate {
            text: s.as_ptr(),
            comment: std::ptr::null(),
            label: std::ptr::null(),
        })
        .collect();

    let composition = typio_abi::TypioComposition {
        struct_size: std::mem::size_of::<typio_abi::TypioComposition>(),
        segments: if segments.is_empty() {
            std::ptr::null()
        } else {
            segments.as_ptr()
        },
        segment_count: segments.len(),
        cursor_pos,
        candidates: if candidates.is_empty() {
            std::ptr::null()
        } else {
            candidates.as_ptr()
        },
        candidate_count: candidates.len(),
        page,
        page_size,
        total,
        selected,
        has_prev,
        has_next,
        content_signature: 0,
        revision: 0,
        host_managed_selection,
    };

    crate::input_context::typio_input_context_set_composition(
        ctx.as_raw() as *mut crate::input_context::TypioInputContext,
        &composition as *const _ as *const crate::types::TypioComposition,
    );
    Ok(())
}

fn decode_hex_list(value: &str) -> Result<Vec<String>> {
    if value.is_empty() {
        return Ok(vec![]);
    }
    value.split(',').map(decode_hex_to_string).collect()
}

fn parse_mode(payload: &str) -> Result<EngineMode> {
    let fields: Vec<&str> = payload.split('\t').collect();
    if fields.len() < 8 {
        return Err(EngineError::Transport("short mode payload".into()));
    }
    // Field 8 (salience) is optional for forward/backward compatibility: a
    // worker that omits it is treated as `Quiet` — "the default is silence".
    let salience = match fields.get(8).copied() {
        Some("1") => ModeSalience::Notable,
        _ => ModeSalience::Quiet,
    };
    Ok(EngineMode {
        id: decode_hex_to_string(fields[0])?,
        label: decode_hex_to_string(fields[1])?,
        display_label: decode_hex_to_option(fields[2])?,
        icon: decode_hex_to_option(fields[3])?,
        profile_id: decode_hex_to_option(fields[4])?,
        profile_label: decode_hex_to_option(fields[5])?,
        description: decode_hex_to_option(fields[6])?,
        is_active: parse_bool(fields[7]),
        salience,
    })
}

fn decode_hex_to_option(value: &str) -> Result<Option<String>> {
    if value.is_empty() {
        Ok(None)
    } else {
        decode_hex_to_string(value).map(Some)
    }
}

fn parse_i32(value: &str) -> i32 {
    value.parse::<i32>().unwrap_or(0)
}

fn parse_u32(value: &str) -> u32 {
    value.parse::<u32>().unwrap_or(0)
}

fn parse_bool(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        return Err(EngineError::Transport("odd-length hex payload".into()));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_value(bytes[i])?;
        let lo = hex_value(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_value(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(EngineError::Transport("invalid hex payload".into())),
    }
}
