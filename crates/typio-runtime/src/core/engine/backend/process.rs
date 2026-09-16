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
    Candidate, Command, Composition, ContextOutput, Engine, EngineAvailability, EngineError,
    EngineInfo, EngineMode, InputContext, InstanceHandle, KeyEvent, KeyProcessResult, KeyState,
    KeyboardEngine, ModeSalience, PreeditFormat, PreeditSegment, Result, VoiceEngine,
};
use super::engine_protocol::{
    Availability as WireAvailability, Composition as WireComposition, ENGINE_PROTOCOL_FD,
    EngineHello, EngineKind, Frame, HostHello, KeyEvent as WireKeyEvent,
    KeyResult as WireKeyResult, KeyState as WireKeyState, MessageType, Mode as WireMode,
    ModeSalience as WireModeSalience, Reply, ReplyRecord, Request, SchemaDefault, SchemaField,
    read_frame, write_frame,
};
use crate::config_schema::{ConfigDefault, ConfigSchemaField, replace_process_engine_schema};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

// HELLO contains metadata and schema only. Heavy engine initialisation starts
// after HostHello and is covered by the `init` request timeout below.
const ENGINE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const ENGINE_KEY_TIMEOUT: Duration = Duration::from_millis(50);
const ENGINE_REQUEST_TIMEOUT: Duration = Duration::from_millis(100);
const ENGINE_INIT_TIMEOUT: Duration = Duration::from_secs(60);
const ENGINE_RELOAD_TIMEOUT: Duration = Duration::from_secs(5);
const ENGINE_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const ENGINE_DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);
const ENGINE_VOICE_TIMEOUT: Duration = Duration::from_secs(120);

/// Respawn backoff: after N consecutive failed recoveries, wait
/// `2^(N-1)` seconds before the next attempt, capped at
/// `2^RECOVERY_BACKOFF_MAX_EXP` seconds (…, 1, 2, 4, … 60).
/// The cap keeps a permanently broken engine retrying roughly once a
/// minute instead of once per keystroke, while a single transient crash
/// still recovers immediately (first retry has no wait).
const RECOVERY_BACKOFF_MAX_EXP: u32 = 6;

/// Pick a timeout appropriate for the request operation.
///
/// `init`/`reload-config` get a bounded slow-operation budget. Cold startup
/// before HELLO is covered separately by [`ENGINE_HANDSHAKE_TIMEOUT`].
/// `process-key` is ultra-tight at 50 ms to guarantee interactive bounded
/// latency on keystrokes; `availability` stays tight at 100 ms.
/// Engine commands share the 5 s control-plane budget. Everything else
/// (focus-in, reset, set-active-mode, commit-candidate, list-modes,
/// get-active-mode) gets a generous 500 ms.
fn request_timeout_for(request: &Request) -> Duration {
    match request.operation() {
        "init" => ENGINE_INIT_TIMEOUT,
        "reload-config" => ENGINE_RELOAD_TIMEOUT,
        "invoke-command" => ENGINE_COMMAND_TIMEOUT,
        "process-key" => ENGINE_KEY_TIMEOUT,
        "availability" => ENGINE_REQUEST_TIMEOUT,
        "process-audio" => ENGINE_VOICE_TIMEOUT,
        _ => ENGINE_DEFAULT_TIMEOUT,
    }
}

/// Out-of-process backend.
#[derive(Debug)]
pub struct ProcessBackend {
    info: EngineInfo,
    argv: Vec<String>,
    runtime_dirs: RuntimeDirs,
    engine: Option<ProcessEngine>,
    /// In-flight asynchronous respawn of a poisoned worker. The respawned
    /// and re-initialised engine arrives on this channel and is installed
    /// by the next [`Self::with_engine`] call. While a recovery is in
    /// flight the backend has no engine, so keyboard keys fall back to
    /// passthrough (`NotHandled`) instead of freezing the host's main loop
    /// on a multi-second spawn + init.
    recovery: Option<std::sync::mpsc::Receiver<Result<ProcessEngine>>>,
    /// Consecutive failed recovery attempts, driving the backoff deadline.
    /// Reset to zero whenever a respawned worker completes its first request
    /// successfully (i.e. the engine is genuinely healthy again).
    recovery_failures: u32,
    /// Earliest instant at which another respawn may be attempted. A worker
    /// that crashes on startup would otherwise be respawned on every key
    /// press — each attempt costing a thread, a process, and a 5 s
    /// handshake — turning a broken engine into an unbounded spawn storm
    /// on the input path.
    recovery_not_before: Option<std::time::Instant>,
    schema_registered: bool,
    #[cfg(test)]
    mock_engine: Option<MockProcessEngine>,
}

#[derive(Debug, Clone, Default)]
struct RuntimeDirs {
    config: String,
    data: String,
    state: String,
}

impl ProcessBackend {
    /// Construct a backend from an executable argv vector.
    pub fn new(info: EngineInfo, argv: Vec<String>) -> Self {
        Self {
            info,
            argv,
            runtime_dirs: RuntimeDirs::default(),
            engine: None,
            recovery: None,
            recovery_failures: 0,
            recovery_not_before: None,
            schema_registered: false,
            #[cfg(test)]
            mock_engine: None,
        }
    }

    /// Construct an in-memory process substitute for registry policy tests.
    /// Real framing and process lifecycle remain covered by this module's
    /// worker test and by `typio-engine-check`.
    #[cfg(test)]
    pub(crate) fn new_mock(info: EngineInfo) -> Self {
        Self {
            mock_engine: Some(MockProcessEngine { info: info.clone() }),
            info,
            argv: Vec::new(),
            runtime_dirs: RuntimeDirs::default(),
            engine: None,
            recovery: None,
            recovery_failures: 0,
            recovery_not_before: None,
            schema_registered: false,
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

    /// Set host-owned runtime directories sent in `HostHello`.
    pub(crate) fn set_runtime_dirs(&mut self, instance: &InstanceHandle) {
        let (config, data, state) = instance.runtime_dirs();
        self.runtime_dirs = RuntimeDirs {
            config: config.to_string(),
            data: data.to_string(),
            state: state.to_string(),
        };
    }

    /// Validate the worker's HELLO and register its configuration schema,
    /// then stop it before heavyweight engine initialisation begins.
    ///
    /// Discovery uses this so strict config validation and settings UIs can
    /// see engine-owned fields before that engine is selected.
    pub fn probe_schema(&mut self) -> Result<()> {
        if self.argv.is_empty() || self.argv[0].is_empty() {
            return Err(EngineError::InvalidArgument);
        }
        ProcessEngine::probe_schema(&self.info, &self.argv)?;
        self.schema_registered = true;
        Ok(())
    }

    /// Start the engine process if needed.
    pub fn instantiate(&mut self) -> Result<()> {
        #[cfg(test)]
        if self.mock_engine.is_some() {
            return Ok(());
        }
        // An explicit (re)instantiation supersedes any in-flight async
        // recovery: a completed recovery that cannot be delivered shuts
        // its worker down on drop. It is a deliberate act (engine reload,
        // registry re-register) so it also clears the failure backoff —
        // the operator has explicitly asked for a fresh attempt now.
        self.recovery = None;
        self.recovery_failures = 0;
        self.recovery_not_before = None;
        // If a previous process was poisoned, drop it before respawning.
        if self
            .engine
            .as_ref()
            .map(|e| e.is_poisoned())
            .unwrap_or(false)
        {
            self.engine.take();
        }
        if self.engine.is_some() {
            return Ok(());
        }
        if self.argv.is_empty() || self.argv[0].is_empty() {
            return Err(EngineError::InvalidArgument);
        }
        self.engine = Some(ProcessEngine::spawn(
            self.info.clone(),
            &self.argv,
            &self.runtime_dirs,
            !self.schema_registered,
        )?);
        self.schema_registered = true;
        Ok(())
    }

    /// Whether the engine process has been started and is healthy.
    pub fn is_instantiated(&self) -> bool {
        #[cfg(test)]
        if self.mock_engine.is_some() {
            return true;
        }
        self.engine
            .as_ref()
            .map(|e| !e.is_poisoned())
            .unwrap_or(false)
    }

    /// Execute a closure with a mutable reference to the engine process.
    ///
    /// If the worker was poisoned by a prior transport error, an
    /// asynchronous respawn is kicked off (or, when already finished,
    /// installed) before the closure runs. While a respawn is still in
    /// flight this returns `None` and the closure does not run: callers on
    /// the keyboard path then treat the key as not handled and forward it
    /// to the application, so a crashed engine cannot freeze the host's
    /// main loop for the duration of a spawn + init (which can take tens
    /// of seconds while the keyboard grab is held). The worker starts
    /// heavyweight engine initialisation after HostHello; the
    /// protocol-level `init` request waits for that startup to finish and
    /// confirms readiness.
    pub fn with_engine<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn Engine) -> R,
    {
        #[cfg(test)]
        if let Some(engine) = self.mock_engine.as_mut() {
            return Some(f(engine));
        }
        self.recover_poisoned();
        let result = self.engine.as_mut().map(|e| f(e));
        if result.is_some() {
            // The request path reached a live worker; any past recovery
            // trouble is definitively over.
            self.note_recovery_success();
        }
        result
    }

    fn recover_poisoned(&mut self) {
        #[cfg(test)]
        if self.mock_engine.is_some() {
            return;
        }
        // Install a completed asynchronous respawn, if one has arrived.
        if let Some(rx) = self.recovery.as_ref() {
            match rx.try_recv() {
                Ok(Ok(engine)) => {
                    log::info!("Engine '{}' respawned asynchronously", self.info.name);
                    self.engine = Some(engine);
                    self.recovery = None;
                }
                Ok(Err(e)) => {
                    log::error!("Engine '{}' async respawn failed: {:?}", self.info.name, e);
                    self.recovery = None;
                    self.record_recovery_failure();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Respawn still in flight.
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.recovery = None;
                }
            }
        }

        let needs_respawn = self
            .engine
            .as_ref()
            .map(|e| e.is_poisoned())
            .unwrap_or(false);
        if !needs_respawn {
            return;
        }
        self.engine.take();
        if self.argv.is_empty() || self.argv[0].is_empty() {
            return;
        }
        if self.recovery.is_some() {
            // A respawn is already in flight; don't pile up workers.
            return;
        }
        // Backoff gate: an engine that keeps failing to come back must not
        // be respawned on every key press. Each failure doubles the wait,
        // capped; the first retry is immediate so a one-off crash (OOM kill,
        // transient exec failure) still recovers instantly.
        if let Some(not_before) = self.recovery_not_before
            && std::time::Instant::now() < not_before
        {
            return;
        }
        // Respawn + re-initialise on a detached thread. Spawn can wait on a
        // 5 s handshake and `Initialize` on a 60 s budget; doing either on
        // the calling (main-loop) thread would freeze key routing while the
        // keyboard grab is held. A completed recovery that nobody collects
        // (backend destroyed meanwhile) shuts its worker down on drop.
        let (tx, rx) = std::sync::mpsc::channel();
        let info = self.info.clone();
        let argv = self.argv.clone();
        let runtime_dirs = self.runtime_dirs.clone();
        let name = self.info.name.clone();
        std::thread::spawn(move || {
            let result = ProcessEngine::spawn(info, &argv, &runtime_dirs, false)
                .and_then(|engine| engine.request(&Request::Initialize, None).map(|_| engine));
            if tx.send(result).is_err() {
                log::debug!("Engine '{}' async respawn result discarded", name);
            }
        });
        self.recovery = Some(rx);
    }

    /// Double the respawn backoff after a failed recovery. The first retry
    /// after a crash stays immediate; repeated failures back off
    /// exponentially up to [`RECOVERY_BACKOFF_CAP`].
    fn record_recovery_failure(&mut self) {
        self.recovery_failures = self.recovery_failures.saturating_add(1);
        let exp = (self.recovery_failures - 1).min(RECOVERY_BACKOFF_MAX_EXP);
        let secs = 1u64 << exp;
        self.recovery_not_before =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(secs));
        log::warn!(
            "Engine '{}' recovery failed {} time(s); next respawn attempt in >= {}s",
            self.info.name,
            self.recovery_failures,
            secs
        );
    }

    /// A successful request through a respawned worker proves the engine is
    /// genuinely healthy again; clear the backoff so a future crash still
    /// recovers instantly.
    fn note_recovery_success(&mut self) {
        if self.recovery_failures != 0 || self.recovery_not_before.is_some() {
            self.recovery_failures = 0;
            self.recovery_not_before = None;
        }
    }

    /// Execute a closure with an immutable reference to the engine process.
    pub fn with_engine_ref<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&dyn Engine) -> R,
    {
        #[cfg(test)]
        if let Some(engine) = self.mock_engine.as_ref() {
            return Some(f(engine));
        }
        self.engine.as_ref().map(|e| f(e))
    }

    /// Stop the engine process.
    pub fn destroy(&mut self) {
        #[cfg(test)]
        self.mock_engine.take();
        self.engine.take();
        // Abandon any in-flight respawn: a completed recovery that cannot
        // be delivered shuts its worker down on drop. Backoff state is
        // cleared too — a destroyed backend has no history.
        self.recovery = None;
        self.recovery_failures = 0;
        self.recovery_not_before = None;
        if self.schema_registered {
            let _ = replace_process_engine_schema(&self.info.name, &[]);
            self.schema_registered = false;
        }
    }

    /// Snapshot the active voice worker for an inference job.
    ///
    /// The returned handle owns the worker's shared transport state, so the
    /// registry may switch or unload the slot without invalidating an
    /// in-flight inference thread.
    pub(crate) fn voice_handle(&mut self) -> Option<VoiceProcessHandle> {
        #[cfg(test)]
        if self.mock_engine.is_some() {
            return None;
        }
        self.recover_poisoned();
        self.engine.as_ref().cloned().map(VoiceProcessHandle)
    }
}

#[cfg(test)]
#[derive(Debug)]
struct MockProcessEngine {
    info: EngineInfo,
}

#[cfg(test)]
impl Engine for MockProcessEngine {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn init(&mut self, _instance: &mut InstanceHandle) -> Result<()> {
        Ok(())
    }

    fn deactivate(&mut self) {}

    fn focus_in(&mut self, _ctx: &mut InputContext) {}

    fn focus_out(&mut self, _ctx: &mut InputContext) {}

    fn reset(&mut self, _ctx: &mut InputContext) {}

    fn reload_config(&mut self) -> Result<()> {
        Ok(())
    }

    fn as_keyboard(&mut self) -> Option<&mut dyn KeyboardEngine> {
        (self.info.engine_type == super::super::EngineType::Keyboard)
            .then_some(self as &mut dyn KeyboardEngine)
    }

    fn as_keyboard_ref(&self) -> Option<&dyn KeyboardEngine> {
        (self.info.engine_type == super::super::EngineType::Keyboard)
            .then_some(self as &dyn KeyboardEngine)
    }

    fn as_voice(&mut self) -> Option<&mut dyn VoiceEngine> {
        (self.info.engine_type == super::super::EngineType::Voice)
            .then_some(self as &mut dyn VoiceEngine)
    }

    fn as_voice_ref(&self) -> Option<&dyn VoiceEngine> {
        (self.info.engine_type == super::super::EngineType::Voice)
            .then_some(self as &dyn VoiceEngine)
    }

    fn list_commands(&self) -> Vec<Command> {
        vec![Command {
            id: "diagnose".into(),
            label: "Run diagnostics".into(),
        }]
    }

    fn invoke_command(&mut self, id: &str) -> Result<()> {
        (id == "diagnose")
            .then_some(())
            .ok_or(EngineError::NotFound)
    }
}

#[cfg(test)]
impl KeyboardEngine for MockProcessEngine {
    fn process_key(&mut self, _ctx: &mut InputContext, _event: &KeyEvent) -> KeyProcessResult {
        KeyProcessResult::NotHandled
    }
}

#[cfg(test)]
impl VoiceEngine for MockProcessEngine {
    fn process_audio(&self, _samples: &[f32]) -> Option<String> {
        None
    }
}

impl Drop for ProcessBackend {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// Thread-safe, owned handle to one running voice worker.
pub struct VoiceProcessHandle(ProcessEngine);

impl VoiceProcessHandle {
    /// Run inference through the captured worker transport.
    pub fn process_audio(&self, samples: &[f32]) -> Option<String> {
        self.0.process_audio(samples)
    }
}

#[derive(Clone)]
struct ProcessEngine {
    info: Arc<EngineInfo>,
    shared: Arc<ProcessEngineShared>,
}

struct ProcessEngineShared {
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
    /// Set when a transport error leaves the socket stream potentially
    /// mid-frame. A poisoned engine must never be reused — the next
    /// `with_engine` call will respawn a fresh process.
    poisoned: bool,
}

impl std::fmt::Debug for ProcessEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessEngine")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// Nonblocking stream adapter with one absolute deadline for a complete frame.
///
/// Socket-level `SO_RCVTIMEO`/`SO_SNDTIMEO` is process-global mutable state on
/// the file description and can be rejected by restricted runtimes. Polling a
/// nonblocking descriptor makes the deadline local to the current transaction
/// and matches the conformance runner's behavior.
struct DeadlineChannel<'a> {
    stream: &'a mut UnixStream,
    deadline: Instant,
}

impl<'a> DeadlineChannel<'a> {
    fn new(stream: &'a mut UnixStream, timeout: Duration) -> Self {
        Self {
            stream,
            deadline: Instant::now() + timeout,
        }
    }

    fn wait(&self, events: libc::c_short) -> io::Result<()> {
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "engine protocol deadline expired",
                ));
            }
            let timeout_ms = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
            let mut descriptor = libc::pollfd {
                fd: self.stream.as_raw_fd(),
                events,
                revents: 0,
            };
            let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
            if result > 0 {
                if descriptor.revents & libc::POLLNVAL != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "engine protocol descriptor became invalid",
                    ));
                }
                return Ok(());
            }
            if result == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "engine protocol deadline expired",
                ));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

impl Read for DeadlineChannel<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.stream.read(buffer) {
                Ok(read) => return Ok(read),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    self.wait(libc::POLLIN)?;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Write for DeadlineChannel<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        loop {
            match self.stream.write(buffer) {
                Ok(written) => return Ok(written),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    self.wait(libc::POLLOUT)?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn read_frame_with_timeout(stream: &mut UnixStream, timeout: Duration) -> Result<Frame> {
    read_frame(&mut DeadlineChannel::new(stream, timeout))
}

fn write_frame_with_timeout(
    stream: &mut UnixStream,
    frame: &Frame,
    timeout: Duration,
) -> Result<()> {
    write_frame(&mut DeadlineChannel::new(stream, timeout), frame)
}

fn launch_worker(argv: &[String]) -> Result<(Child, UnixStream)> {
    let (host_stream, engine_stream) = UnixStream::pair()
        .map_err(|e| EngineError::Transport(format!("engine-protocol socketpair failed: {e}")))?;
    host_stream
        .set_nonblocking(true)
        .map_err(|e| EngineError::Transport(format!("engine-protocol nonblocking setup: {e}")))?;

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

    // ETXTBSY is transient while a package upgrade replaces an executable.
    const SPAWN_RETRIES: u32 = 10;
    let child = {
        let mut attempt = 0;
        loop {
            match command.spawn() {
                Ok(child) => break child,
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < SPAWN_RETRIES => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(EngineError::Transport(format!("spawn {}: {e}", argv[0])));
                }
            }
        }
    };
    drop(engine_stream);
    Ok((child, host_stream))
}

fn stop_child(mut child: Child) {
    // Child deliberately does not kill or reap on Drop. Always clean up a
    // schema probe or failed handshake so discovery cannot leak workers.
    let _ = child.kill();
    let _ = child.wait();
}

fn read_and_register_engine_hello(info: &EngineInfo, stream: &mut UnixStream) -> Result<()> {
    let hello = read_frame_with_timeout(stream, ENGINE_HANDSHAKE_TIMEOUT)?;
    if hello.message_type != MessageType::EngineHello {
        return Err(EngineError::Transport(format!(
            "engine-protocol expected hello, got {:?}",
            hello.message_type
        )));
    }
    let hello = EngineHello::decode(&hello.payload).map_err(protocol_error)?;
    validate_engine_hello(info, &hello)?;
    let fields = hello
        .schema
        .iter()
        .map(HelloSchemaField::from_wire)
        .collect::<Result<Vec<_>>>()?;
    replace_process_engine_schema(&info.name, &fields).map_err(|error| {
        EngineError::Transport(format!(
            "engine-protocol rejected schema for '{}': {error:?}",
            info.name
        ))
    })
}

impl ProcessEngine {
    fn probe_schema(info: &EngineInfo, argv: &[String]) -> Result<()> {
        let (child, mut stream) = launch_worker(argv)?;
        let result = read_and_register_engine_hello(info, &mut stream);
        stop_child(child);
        result
    }

    fn spawn(
        info: EngineInfo,
        argv: &[String],
        runtime_dirs: &RuntimeDirs,
        remove_schema_on_failure: bool,
    ) -> Result<Self> {
        let (child, mut host_stream) = launch_worker(argv)?;

        let handshake = (|| -> Result<()> {
            read_and_register_engine_hello(&info, &mut host_stream)?;

            let host_hello = HostHello {
                engine: info.name.clone(),
                kind: wire_engine_kind(info.engine_type),
                config_dir: runtime_dirs.config.clone(),
                data_dir: runtime_dirs.data.clone(),
                state_dir: runtime_dirs.state.clone(),
            };
            write_frame_with_timeout(
                &mut host_stream,
                &Frame::new(MessageType::HostHello, 0, host_hello.encode()),
                ENGINE_HANDSHAKE_TIMEOUT,
            )?;
            Ok(())
        })();
        if let Err(error) = handshake {
            stop_child(child);
            if remove_schema_on_failure {
                let _ = replace_process_engine_schema(&info.name, &[]);
            }
            return Err(error);
        }

        Ok(Self {
            info: Arc::new(info),
            shared: Arc::new(ProcessEngineShared {
                process: Mutex::new(EngineProcess {
                    child,
                    stream: host_stream,
                    next_request_id: 1,
                    poisoned: false,
                }),
                observed: Mutex::new(ModeObservation::default()),
            }),
        })
    }

    /// Mark the engine as poisoned after a transport error and kill the
    /// child process so it cannot linger. The next `with_engine` call will
    /// detect the poisoned state and respawn a fresh worker.
    fn poison(process: &mut EngineProcess, op: &str, error: &EngineError) {
        if process.poisoned {
            return;
        }
        process.poisoned = true;
        let _ = process.child.kill();
        let _ = process.child.wait();
        log::warn!(
            "engine-protocol transport error during '{op}': {error:?} — \
             worker poisoned, will respawn on next request"
        );
    }

    /// True iff a previous transport error left the socket potentially
    /// mid-frame. A poisoned engine must not be reused.
    fn is_poisoned(&self) -> bool {
        self.shared
            .process
            .lock()
            .map(|p| p.poisoned)
            .unwrap_or(true)
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
        if let Ok(mut observed) = self.shared.observed.lock() {
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

    fn request(&self, request: &Request, ctx: Option<&mut InputContext>) -> Result<WorkerReply> {
        let process = self
            .shared
            .process
            .lock()
            .map_err(|_| EngineError::Transport("worker lock poisoned".into()))?;
        self.request_locked(process, request, ctx)
    }

    /// Send a request only when no other request owns the worker channel.
    /// Main-loop lifecycle operations use this for voice workers so a long
    /// inference never stalls keyboard/Wayland dispatch while waiting on the
    /// transport mutex.
    fn try_request(
        &self,
        request: &Request,
        ctx: Option<&mut InputContext>,
    ) -> Result<Option<WorkerReply>> {
        let process = match self.shared.process.try_lock() {
            Ok(process) => process,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => {
                return Err(EngineError::Transport("worker lock poisoned".into()));
            }
        };
        self.request_locked(process, request, ctx).map(Some)
    }

    fn request_locked(
        &self,
        mut process: MutexGuard<'_, EngineProcess>,
        request: &Request,
        mut ctx: Option<&mut InputContext>,
    ) -> Result<WorkerReply> {
        let operation = request.operation();
        if process.poisoned {
            return Err(EngineError::Transport(format!(
                "engine-protocol worker is poisoned (previous transport error); op='{operation}'"
            )));
        }

        // Poll with a per-request deadline so heavy operations (init/deploy)
        // do not inherit the 100 ms hot-path budget. This avoids mutable
        // socket-level timeout state and remains deterministic under parallel
        // workers and restricted process sandboxes.
        let timeout = request_timeout_for(request);

        let request_id = process.next_request_id;
        process.next_request_id = process.next_request_id.wrapping_add(1).max(1);
        let payload = request.encode();
        let write_result = write_frame_with_timeout(
            &mut process.stream,
            &Frame::new(MessageType::Request, request_id, payload),
            timeout,
        );
        if let Err(ref e) = write_result {
            Self::poison(&mut process, operation, e);
            return Err(e.clone());
        }

        let frame = match read_frame_with_timeout(&mut process.stream, timeout) {
            Ok(f) => f,
            Err(e) => {
                Self::poison(&mut process, operation, &e);
                return Err(e);
            }
        };
        if frame.request_id != request_id {
            let err = EngineError::Transport(format!(
                "engine-protocol response id mismatch: expected {request_id}, got {}",
                frame.request_id
            ));
            Self::poison(&mut process, operation, &err);
            return Err(err);
        }
        if frame.message_type == MessageType::Error {
            let message = String::from_utf8_lossy(&frame.payload);
            return Err(EngineError::Transport(message.into_owned()));
        }
        if frame.message_type != MessageType::Response {
            let err = EngineError::Transport(format!(
                "engine-protocol expected response, got {:?}",
                frame.message_type
            ));
            Self::poison(&mut process, operation, &err);
            return Err(err);
        }

        let mut reply = WorkerReply::default();
        let wire_reply = Reply::decode(&frame.payload).map_err(protocol_error)?;
        for record in wire_reply.records {
            self.handle_response_record(record, &mut reply, ctx.as_deref_mut())?;
        }
        if let Some(err) = reply.error.take() {
            return Err(match err.split('\t').next().unwrap_or("") {
                "NOT_FOUND" => EngineError::NotFound,
                "NOT_SUPPORTED" => EngineError::NotSupported,
                _ => EngineError::Transport(err),
            });
        }
        if let Some(mode) = reply.active_mode.as_ref() {
            self.observe_active_mode(mode);
        }
        Ok(reply)
    }

    fn handle_response_record(
        &self,
        record: ReplyRecord,
        reply: &mut WorkerReply,
        ctx: Option<&mut InputContext>,
    ) -> Result<()> {
        match record {
            ReplyRecord::Ok => {}
            ReplyRecord::Error(error) => reply.error = Some(error),
            ReplyRecord::KeyResult(result) => reply.key_result = Some(key_result_from_wire(result)),
            ReplyRecord::Availability(state) => {
                reply.availability = Some(availability_from_wire(state));
            }
            ReplyRecord::Text(text) => reply.text = Some(text),
            ReplyRecord::Mode(mode) => reply.modes.push(mode_from_wire(mode)),
            ReplyRecord::Command(command) => reply.commands.push(Command {
                id: command.id,
                label: command.label,
            }),
            ReplyRecord::ActiveMode(mode) => reply.active_mode = Some(mode_from_wire(mode)),
            ReplyRecord::Composition(composition) => {
                if let Some(ctx) = ctx {
                    ctx.push_output(ContextOutput::Composition(composition_from_wire(
                        composition,
                    )));
                }
            }
            ReplyRecord::Commit(text) => {
                if let Some(ctx) = ctx {
                    ctx.push_output(ContextOutput::Commit(text));
                }
            }
            ReplyRecord::Clear => {
                if let Some(ctx) = ctx {
                    ctx.push_output(ContextOutput::Clear);
                }
            }
        }
        Ok(())
    }
}

impl Drop for ProcessEngineShared {
    fn drop(&mut self) {
        if let Ok(mut process) = self.process.lock() {
            let request_id = process.next_request_id;
            let _ = write_frame_with_timeout(
                &mut process.stream,
                &Frame::new(MessageType::Request, request_id, Request::Shutdown.encode()),
                ENGINE_DEFAULT_TIMEOUT,
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
        self.request(&Request::Initialize, None).map(|_| ())
    }

    fn deactivate(&mut self) {
        if self.info.engine_type == super::super::EngineType::Voice {
            let _ = self.try_request(&Request::Deactivate, None);
        } else {
            let _ = self.request(&Request::Deactivate, None);
        }
    }

    fn focus_in(&mut self, ctx: &mut InputContext) {
        let request = Request::FocusIn(context_id(ctx));
        let _ = self.request(&request, Some(ctx));
    }

    fn focus_out(&mut self, ctx: &mut InputContext) {
        let request = Request::FocusOut(context_id(ctx));
        let _ = self.request(&request, Some(ctx));
    }

    fn reset(&mut self, ctx: &mut InputContext) {
        let request = Request::Reset(context_id(ctx));
        let _ = self.request(&request, Some(ctx));
    }

    fn reload_config(&mut self) -> Result<()> {
        if self.info.engine_type == super::super::EngineType::Voice {
            return match self.try_request(&Request::ReloadConfig, None)? {
                Some(_) => Ok(()),
                None => Err(EngineError::Transport(
                    "voice worker busy; reload deferred".into(),
                )),
            };
        }
        self.request(&Request::ReloadConfig, None).map(|_| ())
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
        self.request(&Request::ListCommands, None)
            .map(|reply| reply.commands)
            .unwrap_or_default()
    }

    fn invoke_command(&mut self, id: &str) -> Result<()> {
        self.request(&Request::InvokeCommand(id.to_string()), None)
            .map(|_| ())
    }

    fn on_config_change(&mut self, _key: &str, _value: &str) {
        // The daemon owns the configuration file; workers know its exact root
        // from HostHello and expose a versioned full-reload operation. Use that
        // path for live updates. Voice reload is session-managed so it can be
        // deferred across recording/inference.
        if self.info.engine_type == super::super::EngineType::Voice {
            return;
        }
        if let Err(error) = self.request(&Request::ReloadConfig, None) {
            log::warn!("Engine '{}' config reload failed: {error}", self.info.name);
        }
    }

    fn availability(&self) -> EngineAvailability {
        match self.try_request(&Request::Availability, None) {
            Ok(Some(reply)) => reply.availability.unwrap_or(EngineAvailability::Failed),
            Ok(None) => EngineAvailability::Preparing,
            Err(_) => EngineAvailability::Failed,
        }
    }
}

impl KeyboardEngine for ProcessEngine {
    fn process_key(&mut self, ctx: &mut InputContext, event: &KeyEvent) -> KeyProcessResult {
        let request = Request::ProcessKey(WireKeyEvent {
            context_id: context_id(ctx),
            state: match event.state {
                KeyState::Press => WireKeyState::Press,
                KeyState::Release => WireKeyState::Release,
            },
            keycode: event.code,
            keysym: keysym_to_u32(event.sym),
            modifiers: event.modifiers,
            unicode: event.unicode,
            time: event.time,
            is_repeat: event.is_repeat,
            base_keysym: event.base_keysym,
        });
        self.request(&request, Some(ctx))
            .map(|reply| reply.key_result.unwrap_or(KeyProcessResult::NotHandled))
            .unwrap_or(KeyProcessResult::NotHandled)
    }

    fn list_modes(&self) -> Vec<EngineMode> {
        self.request(&Request::ListModes, None)
            .map(|reply| reply.modes)
            .unwrap_or_default()
    }

    fn get_active_mode(&self, ctx: &InputContext) -> Option<EngineMode> {
        self.request(&Request::GetActiveMode(context_id(ctx)), None)
            .ok()
            .and_then(|reply| reply.active_mode)
    }

    fn set_active_mode(&mut self, ctx: &mut InputContext, mode_id: Option<&str>) -> Result<()> {
        let request = Request::SetActiveMode {
            context_id: context_id(ctx),
            mode_id: mode_id.map(str::to_string),
        };
        self.request(&request, Some(ctx)).map(|_| ())
    }

    fn commit_candidate(&mut self, ctx: &mut InputContext, candidate_index: i32) -> Result<()> {
        let request = Request::CommitCandidate {
            context_id: context_id(ctx),
            index: candidate_index,
        };
        self.request(&request, Some(ctx)).map(|_| ())
    }

    fn take_changed_mode(&mut self) -> Option<EngineMode> {
        let mut observed = self.shared.observed.lock().ok()?;
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
        let mut bytes = Vec::with_capacity(std::mem::size_of_val(samples));
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.request(&Request::ProcessAudio(bytes), None)
            .ok()
            .and_then(|reply| reply.text)
    }
}

#[derive(Default)]
struct WorkerReply {
    error: Option<String>,
    key_result: Option<KeyProcessResult>,
    availability: Option<EngineAvailability>,
    text: Option<String>,
    modes: Vec<EngineMode>,
    commands: Vec<Command>,
    active_mode: Option<EngineMode>,
}

fn context_id(ctx: &InputContext) -> u64 {
    ctx.id()
}

fn key_result_from_wire(result: WireKeyResult) -> KeyProcessResult {
    match result {
        WireKeyResult::NotHandled => KeyProcessResult::NotHandled,
        WireKeyResult::Handled => KeyProcessResult::Handled,
        WireKeyResult::PassThrough => KeyProcessResult::PassThrough,
    }
}

fn availability_from_wire(state: WireAvailability) -> EngineAvailability {
    match state {
        WireAvailability::Uninitialized => EngineAvailability::Uninitialized,
        WireAvailability::Preparing => EngineAvailability::Preparing,
        WireAvailability::Ready => EngineAvailability::Ready,
        WireAvailability::Failed => EngineAvailability::Failed,
    }
}

fn wire_engine_kind(engine_type: super::super::EngineType) -> EngineKind {
    match engine_type {
        super::super::EngineType::Keyboard => EngineKind::Keyboard,
        super::super::EngineType::Voice => EngineKind::Voice,
    }
}

fn mode_from_wire(mode: WireMode) -> EngineMode {
    EngineMode {
        id: mode.id,
        label: mode.label,
        display_label: mode.display_label,
        icon: mode.icon,
        profile_id: mode.profile_id,
        profile_label: mode.profile_label,
        description: mode.description,
        is_active: mode.is_active,
        salience: match mode.salience {
            WireModeSalience::Quiet => ModeSalience::Quiet,
            WireModeSalience::Notable => ModeSalience::Notable,
        },
    }
}

fn protocol_error(error: typio_engine_protocol::ProtocolError) -> EngineError {
    EngineError::Transport(format!("engine-protocol invalid payload: {error}"))
}

struct HelloSchemaField;

impl HelloSchemaField {
    fn from_wire(field: &SchemaField) -> Result<ConfigSchemaField> {
        let default = match &field.default {
            SchemaDefault::String(value) => ConfigDefault::String(value.clone()),
            SchemaDefault::Integer(value) => ConfigDefault::Integer(*value),
            SchemaDefault::Boolean(value) => ConfigDefault::Boolean(*value),
            SchemaDefault::Float(value) => ConfigDefault::Float(*value),
        };
        Ok(ConfigSchemaField {
            key: field.key.clone(),
            default,
            label: field.label.clone(),
            section: field.section.clone(),
            minimum: field.minimum,
            maximum: field.maximum,
            step: field.step,
            options: field.options.clone(),
            runtime_property: field.runtime_property.clone(),
        })
    }
}

fn validate_engine_hello(info: &EngineInfo, hello: &EngineHello) -> Result<()> {
    if hello.engine != info.name {
        return Err(EngineError::Transport(format!(
            "engine-protocol engine mismatch: manifest '{}' engine '{}'",
            info.name, hello.engine
        )));
    }
    let expected_kind = wire_engine_kind(info.engine_type);
    if hello.kind != expected_kind {
        return Err(EngineError::Transport(format!(
            "engine-protocol type mismatch: manifest '{}' engine '{}'",
            expected_kind.as_str(),
            hello.kind.as_str()
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

fn composition_from_wire(wire: WireComposition) -> Composition {
    Composition {
        segments: wire
            .segments
            .into_iter()
            .map(|segment| PreeditSegment {
                text: segment.text,
                format: match segment.format {
                    super::engine_protocol::PreeditFormat::None => PreeditFormat::None,
                    super::engine_protocol::PreeditFormat::Underline => PreeditFormat::Underline,
                    super::engine_protocol::PreeditFormat::Highlight => PreeditFormat::Highlight,
                },
            })
            .collect(),
        cursor_pos: wire.cursor_pos,
        candidates: wire
            .candidates
            .into_iter()
            .map(|candidate| Candidate {
                text: candidate.text,
                comment: candidate.comment,
                label: candidate.label,
            })
            .collect(),
        page: wire.page,
        page_size: wire.page_size,
        total: wire.total,
        selected: wire.selected,
        has_prev: wire.has_prev,
        has_next: wire.has_next,
        host_managed_selection: wire.host_managed_selection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn recovery_backoff_doubles_and_caps() {
        // The backoff state machine is pure bookkeeping; drive it directly.
        let info = EngineInfo::new("backoff-test", super::super::super::EngineType::Keyboard);
        let mut backend = ProcessBackend::new(info, vec![]);
        assert!(backend.recovery_not_before.is_none());

        // First failure: no wait yet (the *next* attempt is immediate);
        // subsequent failures double, capped at 2^RECOVERY_BACKOFF_MAX_EXP.
        let mut expected = vec![1u64, 2, 4, 8, 16, 32, 64, 64, 64];
        for want in expected.drain(..) {
            backend.record_recovery_failure();
            let not_before = backend
                .recovery_not_before
                .expect("backoff deadline set after failure");
            let now = std::time::Instant::now();
            let remaining = not_before.saturating_duration_since(now).as_secs();
            // Allow ±1s scheduling slack, but the bucket must match.
            assert!(
                remaining + 1 >= want && remaining <= want + 1,
                "expected ~{want}s backoff, got {remaining}s (failures = {})",
                backend.recovery_failures
            );
        }

        // Success resets everything.
        backend.note_recovery_success();
        assert_eq!(backend.recovery_failures, 0);
        assert!(backend.recovery_not_before.is_none());
    }

    #[test]
    fn recovery_backoff_cleared_by_explicit_reinstantiate_and_destroy() {
        let info = EngineInfo::new("backoff-reset", super::super::super::EngineType::Keyboard);
        let mut backend = ProcessBackend::new(info, vec!["/bin/true".into()]);
        backend.record_recovery_failure();
        backend.record_recovery_failure();
        assert!(backend.recovery_not_before.is_some());

        // destroy() clears the history of a destroyed backend.
        backend.destroy();
        assert_eq!(backend.recovery_failures, 0);
        assert!(backend.recovery_not_before.is_none());
    }

    #[test]
    fn schema_probe_launches_worker_and_cleans_up_registration() {
        let executable = std::env::temp_dir().join(format!(
            "typio-runtime-schema-probe-{}-{}.py",
            std::process::id(),
            std::thread::current().name().unwrap_or("worker")
        ));
        fs::write(
            &executable,
            r#"#!/usr/bin/env python3
import os
import struct

payload = ("protocol\t1.0\n"
           "engine\tprobe_worker\n"
           "type\tkeyboard\n"
           "SCHEMA\t656e67696e65732e70726f62655f776f726b65722e656e61626c6564\t2\t1\t456e61626c6564\t70726f62655f776f726b6572\t0\t0\t0\t\t").encode()
header = struct.pack("!IHHIIQI", 0x54594550, 1, 0, 1, 0, 0, len(payload))
os.write(int(os.environ.get("TYPIO_ENGINE_FD", "3")), header + payload)
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let mut backend = ProcessBackend::new(
            EngineInfo::new("probe_worker", super::super::super::EngineType::Keyboard),
            vec![executable.to_string_lossy().into_owned()],
        );
        backend.probe_schema().unwrap();

        let key = "engines.probe_worker.enabled";
        assert!(crate::config_schema::find(key).is_some());
        drop(backend);
        assert!(crate::config_schema::find(key).is_none());
        let _ = fs::remove_file(executable);
    }

    #[test]
    fn request_timeouts_match_operation_cost() {
        let key = Request::ProcessKey(WireKeyEvent {
            context_id: 1,
            state: WireKeyState::Press,
            keycode: 0,
            keysym: 0,
            modifiers: 0,
            unicode: 0,
            time: 0,
            is_repeat: false,
            base_keysym: 0,
        });
        assert_eq!(request_timeout_for(&key), ENGINE_KEY_TIMEOUT);
        assert_eq!(
            request_timeout_for(&Request::Availability),
            ENGINE_REQUEST_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(&Request::Initialize),
            ENGINE_INIT_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(&Request::ReloadConfig),
            ENGINE_RELOAD_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(&Request::InvokeCommand("diagnose".into())),
            ENGINE_COMMAND_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(&Request::ProcessAudio(vec![0])),
            ENGINE_VOICE_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(&Request::FocusIn(1)),
            ENGINE_DEFAULT_TIMEOUT
        );
    }

    #[test]
    fn command_payload_is_typed_and_bounded() {
        let reply =
            Reply::decode(b"COMMAND\t646961676e6f7365\t52756e20646961676e6f7374696373\nEND")
                .unwrap();
        assert_eq!(
            reply.records,
            vec![ReplyRecord::Command(
                super::super::engine_protocol::Command {
                    id: "diagnose".into(),
                    label: "Run diagnostics".into(),
                }
            )]
        );
        assert!(Reply::decode(b"COMMAND\t\t6c6162656c").is_err());
        assert!(Reply::decode(b"COMMAND\t6964").is_err());
        assert!(Reply::decode(b"COMMAND\tzz\t6c6162656c").is_err());
    }

    #[test]
    fn engine_hello_parses_typed_schema() {
        let info = EngineInfo::new("demo", super::super::super::EngineType::Keyboard);
        let hello = EngineHello {
            engine: "demo".into(),
            kind: EngineKind::Keyboard,
            schema: vec![SchemaField {
                key: "engines.demo.mode".into(),
                default: SchemaDefault::String("alpha".into()),
                label: Some("Mode".into()),
                section: Some("demo".into()),
                minimum: 0,
                maximum: 0,
                step: 0,
                options: vec!["alpha".into(), "beta".into()],
                runtime_property: None,
            }],
        };
        validate_engine_hello(&info, &hello).unwrap();
        let schema = hello
            .schema
            .iter()
            .map(HelloSchemaField::from_wire)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(schema.len(), 1);
        let field = &schema[0];
        assert_eq!(field.key, "engines.demo.mode");
        assert_eq!(field.default, ConfigDefault::String("alpha".into()));
        assert_eq!(field.options, ["alpha", "beta"]);
    }

    #[test]
    fn engine_hello_rejects_malformed_schema() {
        let info = EngineInfo::new("demo", super::super::super::EngineType::Keyboard);
        assert!(
            EngineHello::decode(
                b"protocol\t1.0\nengine\tdemo\ntype\tkeyboard\nSCHEMA\t00\t9\t\t\t\t0\t0\t0\t\t"
            )
            .is_err()
        );

        let mismatch = EngineHello {
            engine: "other".into(),
            kind: EngineKind::Keyboard,
            schema: Vec::new(),
        };
        assert!(validate_engine_hello(&info, &mismatch).is_err());
    }
}
