//! Voice push-to-talk integration.
//!
//! Bridges libtypio's voice-session state machine to a real Linux audio
//! source. The session itself (recording buffer, inference threading,
//! result delivery) lives in libtypio; the host's only job is to supply
//! captured microphone audio and to surface the recognised text.
//!
//! Audio capture is delegated to PipeWire's `pw-record` CLI: on
//! push-to-talk press the session calls the audio source's `start` op,
//! which spawns
//!
//! ```text
//! pw-record --raw --rate 16000 --channels 1 --format f32 -
//! ```
//!
//! and streams its stdout (raw little-endian f32 mono PCM, already
//! resampled to the 16 kHz mono the voice engines expect) into
//! `typio_voice_session_feed_audio`. On release the session calls `stop`,
//! which kills the child; libtypio then runs inference on the buffered
//! samples on its own thread and signals completion via the session
//! eventfd.
//!
//! Using the `pw-record` subprocess (rather than linking libpipewire)
//! keeps the host free of a heavy native dependency and lets PipeWire do
//! the device selection and resampling.

use std::ffi::{CStr, c_void};
use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use typio::types::TypioVoiceSession;
use typio::voice::session::{
    TypioAudioSource, TypioAudioSourceOps, typio_voice_session_dispatch,
    typio_voice_session_feed_audio, typio_voice_session_free, typio_voice_session_get_fd,
    typio_voice_session_get_unavail_reason, typio_voice_session_is_available,
    typio_voice_session_new, typio_voice_session_set_audio_source,
    typio_voice_session_set_callback, typio_voice_session_start, typio_voice_session_stop,
};
use typio::voice::types::{TypioVoiceSessionEvent, TypioVoiceSessionEventType, VoiceState};

/// Outcome drained from the voice session after a `dispatch`.
#[derive(Debug, Clone)]
pub enum VoiceOutcome {
    /// Recognised text (already tag-filtered and trimmed by libtypio).
    Result(String),
    /// Session state transition (Recording / Processing / Idle / Loading).
    State(VoiceState),
    /// An error message from the session.
    Error(String),
}

/// A raw session pointer made `Send` so it can be captured by the audio
/// reader thread. Safe because the pointer is only ever used to call
/// `typio_voice_session_feed_audio`, which libtypio guards internally.
struct SendSession(*mut TypioVoiceSession);
unsafe impl Send for SendSession {}

/// PipeWire-backed audio source. Layout-compatible with
/// [`TypioAudioSource`] (the `ops` pointer is the first field), so a
/// `*mut PwRecordSource` can be handed to libtypio as a
/// `*mut TypioAudioSource`.
#[repr(C)]
struct PwRecordSource {
    /// Must be first: libtypio reads `(*source).ops`.
    ops: *const TypioAudioSourceOps,
    /// Session to feed captured samples into.
    session: *mut TypioVoiceSession,
    /// Live capture state (child process + reader thread).
    capture: Mutex<Capture>,
}

#[derive(Default)]
struct Capture {
    child: Option<Child>,
    reader: Option<JoinHandle<()>>,
}

/// Operation vtable shared by every [`PwRecordSource`]. Function pointers
/// are `Sync`, so a `static` is fine.
static PW_OPS: TypioAudioSourceOps = TypioAudioSourceOps {
    start: Some(pw_start),
    stop: Some(pw_stop),
    free: Some(pw_free),
    get_fd: Some(pw_get_fd),
    dispatch: Some(pw_dispatch),
};

/// Spawn `pw-record` and stream its samples into the session. Returns
/// `false` (capture not started) if the process fails to spawn.
extern "C" fn pw_start(source: *mut TypioAudioSource) -> bool {
    let src = unsafe { &*(source as *const PwRecordSource) };
    let mut cap = src.capture.lock().unwrap();
    if cap.child.is_some() {
        return true; // already recording
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
        Err(e) => {
            tracing::warn!(target: "typio.voice", error = %e, "failed to spawn pw-record");
            return false;
        }
    };

    let Some(stdout) = child.stdout.take() else {
        tracing::warn!(target: "typio.voice", "pw-record produced no stdout pipe");
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };

    let session = SendSession(src.session);
    let reader = std::thread::spawn(move || feed_loop(stdout, session));
    cap.child = Some(child);
    cap.reader = Some(reader);
    true
}

/// Read raw f32 PCM from `pw-record` and forward it to the session for as
/// long as the pipe stays open. The loop ends when the child is killed
/// (the pipe's write end closes and `read` returns 0).
fn feed_loop(mut stdout: ChildStdout, session: SendSession) {
    let session = session.0;
    let mut buf = [0u8; 16384];
    // Bytes left over from a read that did not end on a 4-byte boundary.
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = match stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        carry.extend_from_slice(&buf[..n]);
        let full = carry.len() / 4;
        if full == 0 {
            continue;
        }
        // Decode via from_le_bytes rather than transmuting the byte
        // buffer: `carry` is u8-aligned, so a `*const f32` cast would be
        // unaligned (UB).
        let mut samples = Vec::with_capacity(full);
        for chunk in carry.chunks_exact(4).take(full) {
            samples.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        carry.drain(0..full * 4);
        typio_voice_session_feed_audio(session, samples.as_ptr(), samples.len());
    }
}

/// Kill the recorder and join its reader thread.
extern "C" fn pw_stop(source: *mut TypioAudioSource) {
    let src = unsafe { &*(source as *const PwRecordSource) };
    let (child, reader) = {
        let mut cap = src.capture.lock().unwrap();
        (cap.child.take(), cap.reader.take())
    };
    if let Some(mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(reader) = reader {
        let _ = reader.join();
    }
}

/// Free the source. libtypio calls `stop` before `free`, but stopping
/// again is a harmless no-op (the capture state is already cleared).
extern "C" fn pw_free(source: *mut TypioAudioSource) {
    pw_stop(source);
    drop(unsafe { Box::from_raw(source as *mut PwRecordSource) });
}

/// No pollable fd: samples are pushed from the reader thread.
extern "C" fn pw_get_fd(_source: *mut TypioAudioSource) -> i32 {
    -1
}

extern "C" fn pw_dispatch(_source: *mut TypioAudioSource) {}

/// Voice session event callback. Fires synchronously on the thread that
/// calls `start`/`stop`/`dispatch` (the main loop), so it just forwards
/// each event to the controller's channel for draining.
extern "C" fn voice_event_cb(event: *const TypioVoiceSessionEvent, user_data: *mut c_void) {
    if event.is_null() || user_data.is_null() {
        return;
    }
    let tx = unsafe { &*(user_data as *const Sender<VoiceOutcome>) };
    let ev = unsafe { &*event };
    match ev.type_ {
        TypioVoiceSessionEventType::Result => {
            if !ev.text.is_null() {
                let text = unsafe { CStr::from_ptr(ev.text) }
                    .to_string_lossy()
                    .into_owned();
                // libtypio owns the string and asks the caller to free it.
                typio::string::typio_free_string(ev.text);
                let _ = tx.send(VoiceOutcome::Result(text));
            }
        }
        TypioVoiceSessionEventType::StateChange => {
            let _ = tx.send(VoiceOutcome::State(ev.state));
        }
        TypioVoiceSessionEventType::Error => {
            let msg = if ev.error.is_null() {
                "voice error".to_string()
            } else {
                unsafe { CStr::from_ptr(ev.error) }
                    .to_string_lossy()
                    .into_owned()
            };
            let _ = tx.send(VoiceOutcome::Error(msg));
        }
    }
}

/// Owns the libtypio voice session and the PipeWire audio source, and
/// surfaces recognition results to the host event loop.
pub struct VoiceController {
    session: *mut TypioVoiceSession,
    /// Kept alive for the lifetime of the session: the callback's
    /// `user_data` points at this boxed sender.
    _tx: Box<Sender<VoiceOutcome>>,
    rx: Receiver<VoiceOutcome>,
}

impl VoiceController {
    /// Create a voice session for `instance`, attach a PipeWire audio
    /// source, and register the result callback. Returns `None` if the
    /// session could not be created.
    ///
    /// # Safety
    /// `instance` must be a valid, initialised `*mut TypioInstance`.
    pub fn new(instance: *mut typio::instance::TypioInstance) -> Option<Self> {
        let session = typio_voice_session_new(instance);
        if session.is_null() {
            return None;
        }

        // Attach the PipeWire audio source. Ownership transfers to the
        // session: `typio_voice_session_free` invokes the source's `free`
        // op, which drops the box.
        let source = Box::new(PwRecordSource {
            ops: &PW_OPS as *const TypioAudioSourceOps,
            session,
            capture: Mutex::new(Capture::default()),
        });
        let source_ptr = Box::into_raw(source) as *mut TypioAudioSource;
        typio_voice_session_set_audio_source(session, source_ptr);

        // The boxed sender's address is the callback user_data, so it must
        // not move — keep the box in the controller for the whole session.
        let (tx, rx) = channel::<VoiceOutcome>();
        let tx = Box::new(tx);
        let user_data = (&*tx) as *const Sender<VoiceOutcome> as *mut c_void;
        typio_voice_session_set_callback(session, voice_event_cb, user_data);

        Some(Self {
            session,
            _tx: tx,
            rx,
        })
    }

    /// True if a voice engine is loaded and ready and an audio source is
    /// attached — i.e. starting a recording can actually produce a result.
    pub fn is_available(&self) -> bool {
        typio_voice_session_is_available(self.session)
    }

    /// Human-readable reason the session is unavailable (empty when
    /// available).
    pub fn unavail_reason(&self) -> String {
        let ptr = typio_voice_session_get_unavail_reason(self.session);
        if ptr.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    /// Begin recording. Returns `true` if capture started.
    pub fn start(&self) -> bool {
        typio_voice_session_start(self.session)
    }

    /// Stop recording and launch inference.
    pub fn stop(&self) {
        typio_voice_session_stop(self.session);
    }

    /// Pollable eventfd that becomes readable when inference completes,
    /// or -1 if unavailable.
    pub fn session_fd(&self) -> i32 {
        typio_voice_session_get_fd(self.session)
    }

    /// Dispatch pending session events (call when [`Self::session_fd`] is
    /// readable). Fires the callback synchronously, queuing outcomes for
    /// [`Self::drain`].
    pub fn dispatch(&self) {
        typio_voice_session_dispatch(self.session);
    }

    /// Drain all queued outcomes accumulated since the last drain.
    pub fn drain(&self) -> Vec<VoiceOutcome> {
        self.rx.try_iter().collect()
    }
}

impl Drop for VoiceController {
    fn drop(&mut self) {
        if !self.session.is_null() {
            // Frees the session, which stops + frees the audio source and
            // joins any in-flight inference thread. No callbacks fire from
            // here, so the boxed sender is safe to drop afterwards.
            typio_voice_session_free(self.session);
            self.session = std::ptr::null_mut();
        }
    }
}
