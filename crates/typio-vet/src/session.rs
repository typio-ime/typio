use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use typio_engine_protocol::{
    ENGINE_PROTOCOL_FD, EngineHello, EngineKind, Frame, HostHello, MessageType, Reply, Request,
    read_frame, write_frame,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const INIT_TIMEOUT: Duration = Duration::from_secs(60);
const VOICE_TIMEOUT: Duration = Duration::from_secs(120);
const HOT_PATH_TIMEOUT: Duration = Duration::from_millis(500);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct EngineSession {
    child: Option<Child>,
    stream: UnixStream,
    next_request_id: u64,
    runtime: VetRuntime,
}

struct VetRuntime {
    _root: tempfile::TempDir,
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
}

impl VetRuntime {
    fn create() -> Result<Self, String> {
        let root = tempfile::Builder::new()
            .prefix("typio-vet-")
            .tempdir()
            .map_err(|error| format!("create isolated runtime directory: {error}"))?;
        let config_home = root.path().join("config");
        let data_home = root.path().join("data");
        let state_home = root.path().join("state");
        let config_dir = config_home.join("typio");
        let data_dir = data_home.join("typio");
        let state_dir = state_home.join("typio");
        for path in [&config_dir, &data_dir, &state_dir] {
            std::fs::create_dir_all(path)
                .map_err(|error| format!("create {}: {error}", path.display()))?;
        }
        Ok(Self {
            _root: root,
            config_dir,
            data_dir,
            state_dir,
        })
    }

    fn xdg_home(path: &std::path::Path) -> &std::path::Path {
        path.parent().expect("Typio runtime directory has a parent")
    }
}

impl EngineSession {
    pub(crate) fn spawn(argv: &[String]) -> Result<(Self, EngineHello), String> {
        let executable = argv
            .first()
            .filter(|value| !value.is_empty())
            .ok_or("empty engine command")?;
        let (host_stream, engine_stream) = UnixStream::pair()
            .map_err(|error| format!("engine-protocol socketpair failed: {error}"))?;
        host_stream
            .set_nonblocking(true)
            .map_err(|error| format!("set engine channel nonblocking: {error}"))?;
        let runtime = VetRuntime::create()?;

        let engine_fd = engine_stream.as_raw_fd();
        let mut command = Command::new(executable);
        command
            .args(&argv[1..])
            .env("TYPIO_ENGINE_PROTOCOL", "1.0")
            .env("TYPIO_ENGINE_FD", ENGINE_PROTOCOL_FD.to_string())
            .env("XDG_CONFIG_HOME", VetRuntime::xdg_home(&runtime.config_dir))
            .env("XDG_DATA_HOME", VetRuntime::xdg_home(&runtime.data_dir))
            .env("XDG_STATE_HOME", VetRuntime::xdg_home(&runtime.state_dir))
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

        let child = command
            .spawn()
            .map_err(|error| format!("spawn {executable}: {error}"))?;
        drop(engine_stream);

        let mut session = Self {
            child: Some(child),
            stream: host_stream,
            next_request_id: 1,
            runtime,
        };
        let frame = session.read_frame(HANDSHAKE_TIMEOUT).inspect_err(|_| {
            session.terminate();
        })?;
        if frame.message_type != MessageType::EngineHello || frame.request_id != 0 {
            session.terminate();
            return Err(format!(
                "expected request-id 0 ENGINE_HELLO, got id {} {:?}",
                frame.request_id, frame.message_type
            ));
        }
        let hello = EngineHello::decode(&frame.payload).inspect_err(|_| {
            session.terminate();
        });
        let hello = hello.map_err(|error| format!("invalid ENGINE_HELLO: {error}"))?;
        Ok((session, hello))
    }

    pub(crate) fn send_host_hello(&mut self, engine: &str, kind: EngineKind) -> Result<(), String> {
        let hello = HostHello {
            engine: engine.to_string(),
            kind,
            config_dir: self.runtime.config_dir.to_string_lossy().into_owned(),
            data_dir: self.runtime.data_dir.to_string_lossy().into_owned(),
            state_dir: self.runtime.state_dir.to_string_lossy().into_owned(),
        };
        self.write_frame(
            &Frame::new(MessageType::HostHello, 0, hello.encode()),
            HANDSHAKE_TIMEOUT,
        )
    }

    pub(crate) fn request(&mut self, request: &Request) -> Result<Reply, String> {
        let timeout = request_timeout(request);
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.write_frame(
            &Frame::new(MessageType::Request, request_id, request.encode()),
            timeout,
        )?;
        let frame = self.read_frame(timeout)?;
        if frame.request_id != request_id {
            return Err(format!(
                "response id mismatch: expected {request_id}, got {}",
                frame.request_id
            ));
        }
        if frame.message_type == MessageType::Error {
            return Err(format!(
                "engine protocol error: {}",
                String::from_utf8_lossy(&frame.payload)
            ));
        }
        if frame.message_type != MessageType::Response {
            return Err(format!("expected RESPONSE, got {:?}", frame.message_type));
        }
        Reply::decode(&frame.payload).map_err(|error| format!("invalid RESPONSE: {error}"))
    }

    pub(crate) fn stop(&mut self) -> Result<ExitStatus, String> {
        if self.child.is_none() {
            return Err("engine process is already stopped".to_string());
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.write_frame(
            &Frame::new(MessageType::Request, request_id, Request::Shutdown.encode()),
            CONTROL_TIMEOUT,
        )?;

        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        loop {
            let child = self.child.as_mut().expect("checked above");
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    return Ok(status);
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    self.terminate();
                    return Err("engine did not exit within 2 seconds of shutdown".to_string());
                }
                Err(error) => {
                    self.terminate();
                    return Err(format!("wait for engine shutdown: {error}"));
                }
            }
        }
    }

    fn read_frame(&mut self, timeout: Duration) -> Result<Frame, String> {
        let mut channel = DeadlineChannel::new(&mut self.stream, timeout);
        read_frame(&mut channel).map_err(|error| format!("read engine frame: {error}"))
    }

    fn write_frame(&mut self, frame: &Frame, timeout: Duration) -> Result<(), String> {
        let mut channel = DeadlineChannel::new(&mut self.stream, timeout);
        write_frame(&mut channel, frame).map_err(|error| format!("write engine frame: {error}"))
    }

    fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for EngineSession {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn request_timeout(request: &Request) -> Duration {
    match request {
        Request::Initialize => INIT_TIMEOUT,
        Request::ProcessAudio(_) => VOICE_TIMEOUT,
        Request::ProcessKey(_) | Request::Availability => HOT_PATH_TIMEOUT,
        Request::ReloadConfig | Request::InvokeCommand(_) => CONTROL_TIMEOUT,
        _ => HOT_PATH_TIMEOUT,
    }
}

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
