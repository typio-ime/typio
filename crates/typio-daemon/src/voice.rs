//! Voice push-to-talk integration.
//!
//! The host supplies a PipeWire-backed [`AudioSource`]; typio-runtime owns the
//! recording buffer and inference state machine. Engine inference still uses
//! the private Engine Protocol process channel.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;

use typio_runtime::voice::session::{AudioSink, AudioSource, VoiceSession};
use typio_runtime::voice::types::{VoiceEvent, VoiceState};

use crate::runtime::SharedInstance;

/// Outcome drained from the voice session after a `dispatch`.
#[derive(Debug, Clone)]
pub enum VoiceOutcome {
    /// Recognised text, tag-filtered and trimmed by typio-runtime.
    Result(String),
    /// Session state transition.
    State(VoiceState),
    /// An error message from the session.
    Error(String),
}

/// PipeWire-backed audio capture using the lightweight `pw-record` CLI.
#[derive(Default)]
struct PwRecordSource {
    child: Option<Child>,
    reader: Option<JoinHandle<()>>,
}

impl AudioSource for PwRecordSource {
    fn start(&mut self, sink: AudioSink) -> bool {
        if self.child.is_some() {
            return true;
        }
        let mut child = match Command::new("pw-record")
            .args([
                "--raw",
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "f32",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(target: "typio.voice", %error, "failed to spawn pw-record");
                return false;
            }
        };
        let Some(stdout) = child.stdout.take() else {
            tracing::warn!(target: "typio.voice", "pw-record produced no stdout pipe");
            let _ = child.kill();
            let _ = child.wait();
            return false;
        };
        self.reader = Some(std::thread::spawn(move || feed_loop(stdout, sink)));
        self.child = Some(child);
        true
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for PwRecordSource {
    fn drop(&mut self) {
        self.stop();
    }
}

fn feed_loop(mut stdout: ChildStdout, sink: AudioSink) {
    let mut buffer = [0u8; 16_384];
    let mut carry = Vec::new();
    loop {
        let count = match stdout.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        carry.extend_from_slice(&buffer[..count]);
        let complete = carry.len() / 4;
        if complete == 0 {
            continue;
        }
        let samples: Vec<f32> = carry
            .chunks_exact(4)
            .take(complete)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        carry.drain(..complete * 4);
        sink.feed(&samples);
    }
}

/// Owns the Rust voice session and accesses the main-loop registry only when
/// selecting or checking the active worker.
pub struct VoiceController {
    instance: SharedInstance,
    session: Arc<VoiceSession>,
}

impl VoiceController {
    /// Create a voice session and attach PipeWire capture.
    pub fn new(instance: SharedInstance) -> Option<Self> {
        let session = VoiceSession::new(Box::<PwRecordSource>::default()).ok()?;
        Some(Self { instance, session })
    }

    /// Whether an active voice worker is ready.
    pub fn is_available(&self) -> bool {
        self.instance
            .borrow_mut()
            .registry_rust_mut()
            .is_some_and(|mut registry| registry.recovering_active_voice_is_ready())
    }

    /// Human-readable reason the session is unavailable.
    pub fn unavail_reason(&self) -> String {
        let instance = self.instance.borrow();
        let Some(mut registry) = instance.registry_rust_mut() else {
            return "no engine registry".into();
        };
        if registry.active_voice_name().is_none() {
            "no active voice engine".into()
        } else if !registry.recovering_active_voice_is_ready() {
            "voice engine not ready (model not loaded)".into()
        } else {
            String::new()
        }
    }

    /// Begin recording against a snapshot of the current voice worker.
    pub fn start(&self) -> bool {
        let target = self
            .instance
            .borrow_mut()
            .registry_rust_mut()
            .and_then(|mut registry| registry.snapshot_active_voice());
        target.is_some_and(|target| self.session.start(target))
    }

    /// Stop recording and launch inference.
    pub fn stop(&self) {
        self.session.stop();
    }

    /// Pollable eventfd that becomes readable when inference completes.
    pub fn session_fd(&self) -> i32 {
        self.session.fd()
    }

    /// Dispatch inference completion.
    pub fn dispatch(&self) {
        self.session.dispatch();
    }

    /// Drain all queued outcomes accumulated since the last drain.
    pub fn drain(&self) -> Vec<VoiceOutcome> {
        self.session
            .drain_events()
            .into_iter()
            .map(|event| match event {
                VoiceEvent::Result(text) => VoiceOutcome::Result(text),
                VoiceEvent::StateChange(state) => VoiceOutcome::State(state),
                VoiceEvent::Error(error) => VoiceOutcome::Error(error),
            })
            .collect()
    }
}
