//! Voice session — state machine, threading, audio buffering.
//!
//! Replaces `voice_session.c`.  All business logic is now Rust-native;
//! only the backend inference calls cross the FFI boundary.

use crate::core::engine::backend::process::VoiceProcessHandle;
use crate::instance::TypioInstance;
use crate::types::TypioVoiceSession;
use crate::voice::audio::{INITIAL_BUFFER_SAMPLES, MAX_BUFFER_SAMPLES, prepare_audio};
use crate::voice::types::{
    TypioVoiceSessionEvent, TypioVoiceSessionEventCallback, TypioVoiceSessionEventType, VoiceState,
};
use nix::sys::eventfd::{EfdFlags, EventFd};
use std::ffi::{CStr, CString, c_char, c_void};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Shared voice session state. Thread-safe via interior mutability.
pub struct VoiceSession {
    pub(crate) _instance: AtomicPtr<TypioInstance>,
    pub(crate) audio_source: AtomicPtr<TypioAudioSource>,
    pub(crate) state: Mutex<VoiceState>,
    pub(crate) reload_pending: AtomicBool,
    pub(crate) audio_truncated: AtomicBool,
    pub(crate) audio_buffer: Mutex<Vec<f32>>,
    pub(crate) inference_target: Mutex<Option<VoiceProcessHandle>>,
    pub(crate) infer_handle: Mutex<Option<thread::JoinHandle<Option<String>>>>,
    pub(crate) event_fd: Mutex<Option<nix::sys::eventfd::EventFd>>,
    pub(crate) _result: Mutex<Option<String>>,
    pub(crate) callback: Mutex<Option<TypioVoiceSessionEventCallback>>,
    pub(crate) callback_user_data: AtomicPtr<c_void>,
    pub(crate) auto_start_on_load: AtomicBool,
}

// SAFETY: VoiceSession is designed to be shared across threads.
// The raw pointers inside are only accessed under Mutex or AtomicPtr.
unsafe impl Send for VoiceSession {}
unsafe impl Sync for VoiceSession {}

/// Audio source abstraction (injected by frontend).
#[repr(C)]
pub struct TypioAudioSource {
    /// Operation vtable.
    pub ops: *const TypioAudioSourceOps,
}

/// Audio source operation vtable.
#[repr(C)]
pub struct TypioAudioSourceOps {
    /// Start capturing audio.
    pub start: Option<extern "C" fn(*mut TypioAudioSource) -> bool>,
    /// Stop capturing audio.
    pub stop: Option<extern "C" fn(*mut TypioAudioSource)>,
    /// Free the audio source.
    pub free: Option<extern "C" fn(*mut TypioAudioSource)>,
    /// Return a pollable file descriptor, or -1.
    pub get_fd: Option<extern "C" fn(*mut TypioAudioSource) -> i32>,
    /// Dispatch pending audio data.
    pub dispatch: Option<extern "C" fn(*mut TypioAudioSource)>,
}

/* ── C ABI ─────────────────────────────────────────────────────────────── */

/// Create a new voice session associated with the given instance.
///
/// Returns a pointer that must be freed with `typio_voice_session_free`.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_new(instance: *mut TypioInstance) -> *mut TypioVoiceSession {
    if instance.is_null() {
        return std::ptr::null_mut();
    }
    let event_fd =
        match EventFd::from_value_and_flags(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK) {
            Ok(efd) => efd,
            Err(_) => {
                log::error!("Failed to create eventfd");
                return std::ptr::null_mut();
            }
        };

    let session = Arc::new(VoiceSession {
        _instance: AtomicPtr::new(instance),
        audio_source: AtomicPtr::new(std::ptr::null_mut()),
        state: Mutex::new(VoiceState::Idle),
        reload_pending: AtomicBool::new(false),
        audio_truncated: AtomicBool::new(false),
        audio_buffer: Mutex::new(Vec::with_capacity(INITIAL_BUFFER_SAMPLES)),
        inference_target: Mutex::new(None),
        infer_handle: Mutex::new(None),
        event_fd: Mutex::new(Some(event_fd)),
        _result: Mutex::new(None),
        callback: Mutex::new(None),
        callback_user_data: AtomicPtr::new(std::ptr::null_mut()),
        auto_start_on_load: AtomicBool::new(false),
    });

    let raw = Arc::into_raw(session) as *mut TypioVoiceSession;
    unsafe {
        (*instance).voice_session = crate::wrappers::VoiceSessionPtr(raw);
    }
    raw
}

/// Free a voice session and all associated resources.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_free(session: *mut TypioVoiceSession) {
    if session.is_null() {
        return;
    }
    let session = unsafe { Arc::from_raw(session as *const VoiceSession) };
    let instance = session._instance.load(Ordering::SeqCst);
    if !instance.is_null()
        && unsafe { (*instance).voice_session.0 } == session.as_ref() as *const _ as *mut _
    {
        unsafe {
            (*instance).voice_session = crate::wrappers::VoiceSessionPtr(std::ptr::null_mut());
        }
    }
    // Stop audio source.
    let source = session.audio_source.load(Ordering::SeqCst);
    if !source.is_null() {
        unsafe {
            let ops = &*(*source).ops;
            if let Some(stop_fn) = ops.stop {
                stop_fn(source);
            }
            if let Some(free_fn) = ops.free {
                free_fn(source);
            }
        }
    }
    // Wait for inference thread if running.
    let mut infer = session.infer_handle.lock().unwrap();
    if let Some(handle) = infer.take() {
        let _ = handle.join();
    }
    let _ = session.event_fd.lock().unwrap().take();
}

/// Attach an audio source to the voice session.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_set_audio_source(
    session: *mut TypioVoiceSession,
    source: *mut TypioAudioSource,
) {
    if session.is_null() {
        return;
    }
    let session = unsafe { &*(session as *const VoiceSession) };
    let old = session.audio_source.swap(source, Ordering::SeqCst);
    if old.is_null() || old == source {
        return;
    }
    unsafe {
        let ops = &*(*old).ops;
        if let Some(stop_fn) = ops.stop {
            stop_fn(old);
        }
        if let Some(free_fn) = ops.free {
            free_fn(old);
        }
    }
}

/// Set the event callback and user data for voice session notifications.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_set_callback(
    session: *mut TypioVoiceSession,
    callback: TypioVoiceSessionEventCallback,
    user_data: *mut c_void,
) {
    if session.is_null() {
        return;
    }
    let session = unsafe { &*(session as *const VoiceSession) };
    *session.callback.lock().unwrap() = Some(callback);
    session
        .callback_user_data
        .store(user_data, Ordering::SeqCst);
}

/// Start voice recording.
///
/// Returns true if recording began successfully.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_start(session: *mut TypioVoiceSession) -> bool {
    if session.is_null() {
        return false;
    }
    let session = unsafe { &*(session as *const VoiceSession) };

    let mut state = session.state.lock().unwrap();

    if *state == VoiceState::Loading {
        return true;
    }

    if *state != VoiceState::Idle {
        return false;
    }

    let source = session.audio_source.load(Ordering::SeqCst);
    if source.is_null() {
        return false;
    }

    let Some(target) = snapshot_voice_target(session) else {
        return false;
    };

    let started = unsafe {
        let ops = &*(*source).ops;
        if let Some(start_fn) = ops.start {
            start_fn(source)
        } else {
            false
        }
    };

    if !started {
        return false;
    }

    session.audio_buffer.lock().unwrap().clear();
    session.audio_truncated.store(false, Ordering::SeqCst);
    *session.inference_target.lock().unwrap() = Some(target);
    *state = VoiceState::Recording;
    drop(state);
    session.fire_state_change(VoiceState::Recording);
    true
}

/// Stop voice recording and launch inference.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_stop(session: *mut TypioVoiceSession) {
    if session.is_null() {
        return;
    }
    let session = unsafe { &*(session as *const VoiceSession) };

    let mut state = session.state.lock().unwrap();

    if *state == VoiceState::Loading {
        session.auto_start_on_load.store(false, Ordering::SeqCst);
        *state = VoiceState::Idle;
        drop(state);
        session.fire_state_change(VoiceState::Idle);
        return;
    }

    if *state != VoiceState::Recording {
        drop(state);
        return;
    }

    *state = VoiceState::Processing;
    let sample_count = session.audio_buffer.lock().unwrap().len();
    let target = session.inference_target.lock().unwrap().take();
    let audio_truncated = session.audio_truncated.swap(false, Ordering::SeqCst);
    drop(state);

    let source = session.audio_source.load(Ordering::SeqCst);
    if !source.is_null() {
        unsafe {
            let ops = &*(*source).ops;
            if let Some(stop_fn) = ops.stop {
                stop_fn(source);
            }
        }
    }

    log::info!(
        "Voice recording stopped, starting inference ({} samples)",
        sample_count
    );
    if audio_truncated {
        log::warn!("Voice recording exceeded 60 seconds; trailing audio was discarded");
    }
    session.fire_state_change(VoiceState::Processing);

    // Launch inference thread.
    let session_arc = unsafe { Arc::from_raw(session as *const VoiceSession) };
    // Leak the Arc back so the C side keeps ownership; the thread clones its own Arc.
    let _ = Arc::into_raw(session_arc.clone());

    let handle = thread::spawn(move || {
        let mut audio = {
            let mut buf = session_arc.audio_buffer.lock().unwrap();
            std::mem::take(&mut *buf)
        };
        let audio = prepare_audio(&mut audio);
        let result = if audio.is_empty() {
            log::warn!("Voice inference: no audio captured or usable");
            None
        } else {
            run_voice_inference(target, audio)
        };

        // Wake the event loop so dispatch() joins this thread and delivers the
        // result. Without this signal the session stays stuck in Processing and
        // the indicator never clears.
        let fd = session_arc
            .event_fd
            .lock()
            .unwrap()
            .as_ref()
            .map(|efd| efd.as_raw_fd())
            .unwrap_or(-1);
        if fd >= 0 {
            let val: u64 = 1;
            unsafe {
                let _ = libc::write(fd, &val as *const _ as *const libc::c_void, 8);
            }
        }

        result
    });

    *session.infer_handle.lock().unwrap() = Some(handle);
}

/// Inference body extracted so the closure body stays small and readable.
fn snapshot_voice_target(session: &VoiceSession) -> Option<VoiceProcessHandle> {
    let instance = session._instance.load(Ordering::SeqCst);
    if instance.is_null() {
        return None;
    }
    let registry = unsafe { (*instance).registry.0 };
    if registry.is_null() {
        return None;
    }
    unsafe { (*registry).inner.snapshot_active_voice() }
}

fn run_voice_inference(target: Option<VoiceProcessHandle>, audio: Vec<f32>) -> Option<String> {
    let target = target?;
    log::info!("Voice inference: processing {} samples", audio.len());
    target.process_audio(&audio)
}

/// Return the eventfd for polling, or -1 if unavailable.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_get_fd(session: *mut TypioVoiceSession) -> i32 {
    if session.is_null() {
        return -1;
    }
    let session = unsafe { &*(session as *const VoiceSession) };
    session
        .event_fd
        .lock()
        .unwrap()
        .as_ref()
        .map(|efd| efd.as_raw_fd())
        .unwrap_or(-1)
}

/// Dispatch pending voice session events (inference completion, async load).
///
/// Must be called from the event loop when the session fd becomes readable.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_dispatch(session: *mut TypioVoiceSession) {
    if session.is_null() {
        return;
    }
    let session = unsafe { &*(session as *const VoiceSession) };

    // Read and clear eventfd.
    let mut buf = [0u8; 8];
    let event_fd = session
        .event_fd
        .lock()
        .unwrap()
        .as_ref()
        .map(|efd| efd.as_raw_fd())
        .unwrap_or(-1);
    if event_fd >= 0 {
        unsafe {
            let _ = libc::read(event_fd, buf.as_mut_ptr() as *mut libc::c_void, 8);
        }
    }

    // Handle async model load completion.
    // Legacy direct voice-engine path removed; no engine is loaded locally.
    {
        let state = session.state.lock().unwrap();
        if *state == VoiceState::Loading {
            session.auto_start_on_load.store(false, Ordering::SeqCst);
            drop(state);

            *session.state.lock().unwrap() = VoiceState::Idle;
            let event = TypioVoiceSessionEvent {
                type_: TypioVoiceSessionEventType::Error,
                state: VoiceState::Idle,
                text: std::ptr::null_mut(),
                error: c"Voice model failed to load".as_ptr(),
            };
            session.fire_event(event);
            return;
        }
    }

    // Join inference thread.
    let text = {
        let mut infer = session.infer_handle.lock().unwrap();
        if let Some(handle) = infer.take() {
            handle.join().unwrap_or_default()
        } else {
            None
        }
    };

    let mut state = session.state.lock().unwrap();
    *state = VoiceState::Idle;
    let reload_pending = session.reload_pending.load(Ordering::SeqCst);
    session.reload_pending.store(false, Ordering::SeqCst);
    drop(state);

    if reload_pending {
        do_reload_engine(session);
    }

    if let Some(text) = text.filter(|t| !t.is_empty()) {
        let filtered = filter_tags(&text);
        let trimmed = filtered.trim().to_string();
        if !trimmed.is_empty() {
            log::debug!("Voice inference completed ({} UTF-8 bytes)", trimmed.len());
        }
        let c_text = CString::new(trimmed).unwrap_or_else(|_| CString::new("").unwrap());
        let event = TypioVoiceSessionEvent {
            type_: TypioVoiceSessionEventType::Result,
            state: VoiceState::Idle,
            text: c_text.into_raw(),
            error: std::ptr::null(),
        };
        session.fire_event(event);
        session.fire_state_change(VoiceState::Idle);
    } else {
        session.fire_state_change(VoiceState::Idle);
    }
}

/// Return true if a voice engine is active, ready, and an audio source is attached.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_is_available(session: *const TypioVoiceSession) -> bool {
    if session.is_null() {
        return false;
    }
    let session = unsafe { &*(session as *const VoiceSession) };
    let instance = session._instance.load(Ordering::SeqCst);
    if instance.is_null() {
        return false;
    }
    let registry = unsafe { (*instance).registry.0 };
    if registry.is_null() {
        return false;
    }
    let has_source = !session.audio_source.load(Ordering::SeqCst).is_null();
    has_source && unsafe { (*registry).inner.recovering_active_voice_is_ready() }
}

/// Return a static error message explaining why voice is unavailable.
///
/// Returns an empty string if voice is available. Caller must NOT free the
/// returned pointer.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_get_unavail_reason(
    session: *const TypioVoiceSession,
) -> *const c_char {
    if session.is_null() {
        return c"voice session not created".as_ptr();
    }
    let session = unsafe { &*(session as *const VoiceSession) };
    let instance = session._instance.load(Ordering::SeqCst);
    if instance.is_null() {
        return c"no instance".as_ptr();
    }
    let registry = unsafe { (*instance).registry.0 };
    if registry.is_null() {
        return c"no registry".as_ptr();
    }
    if session.audio_source.load(Ordering::SeqCst).is_null() {
        return c"no audio source".as_ptr();
    }
    if !unsafe { (*registry).inner.recovering_active_voice_is_ready() } {
        return c"voice engine not ready (model not loaded)".as_ptr();
    }
    c"".as_ptr()
}

fn do_reload_engine(session: &VoiceSession) {
    let instance = session._instance.load(Ordering::SeqCst);
    if instance.is_null() {
        return;
    }
    let registry = unsafe { (*instance).registry.0 };
    if registry.is_null() {
        return;
    }
    unsafe {
        let r = (*registry)
            .inner
            .with_active_voice_mut(|v| v.reload_config());
        log::info!("Voice engine reload result: {:?}", r);
    }
}

/// Request a reload of the active voice engine config.
///
/// If the session is busy, the reload is deferred until idle.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_reload_engine(session: *mut TypioVoiceSession) {
    if session.is_null() {
        return;
    }
    let session = unsafe { &*(session as *const VoiceSession) };
    let state = session.state.lock().unwrap();
    if *state != VoiceState::Idle {
        session.reload_pending.store(true, Ordering::SeqCst);
        drop(state);
        log::info!("Voice reload deferred: session busy");
        return;
    }
    drop(state);
    do_reload_engine(session);
}

/// Feed audio samples into the session buffer while recording.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_session_feed_audio(
    session: *mut TypioVoiceSession,
    samples: *const f32,
    count: usize,
) {
    if session.is_null() || samples.is_null() || count == 0 {
        return;
    }
    let session = unsafe { &*(session as *const VoiceSession) };
    let state = session.state.lock().unwrap();
    if *state != VoiceState::Recording {
        drop(state);
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts(samples, count) };
    let mut buf = session.audio_buffer.lock().unwrap();
    if append_audio_bounded(&mut buf, slice) {
        session.audio_truncated.store(true, Ordering::SeqCst);
    }
    drop(buf);
    drop(state);
}

fn append_audio_bounded(buf: &mut Vec<f32>, samples: &[f32]) -> bool {
    let remaining = MAX_BUFFER_SAMPLES.saturating_sub(buf.len());
    let accepted = remaining.min(samples.len());
    buf.extend_from_slice(&samples[..accepted]);
    accepted < samples.len()
}

/// Remove bracketed tags (e.g. `[tag]`) from text in place.
#[unsafe(no_mangle)]
pub extern "C" fn typio_voice_filter_tags_inplace(text: *mut c_char) {
    if text.is_null() {
        return;
    }
    unsafe {
        let s = CStr::from_ptr(text).to_string_lossy().into_owned();
        let filtered = filter_tags(&s);
        let bytes = filtered.as_bytes();
        let len = bytes.len();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), text as *mut u8, len);
        *text.add(len) = 0;
    }
}

fn filter_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '[' {
            let mut candidate = String::from("[");
            let mut closed = false;
            for c in chars.by_ref() {
                candidate.push(c);
                if c == ']' {
                    closed = true;
                    break;
                }
            }
            if closed {
                // Skip spaces after tag.
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                if !result.is_empty() && chars.peek().is_some() && !result.ends_with(' ') {
                    result.push(' ');
                }
            } else {
                result.push_str(&candidate);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_tags_preserves_unclosed_bracket_text() {
        assert_eq!(
            filter_tags("hello [unfinished text"),
            "hello [unfinished text"
        );
    }

    #[test]
    fn filter_tags_removes_closed_tags_without_joining_words() {
        assert_eq!(filter_tags("hello [noise] world"), "hello world");
        assert_eq!(filter_tags("hello[noise]world"), "hello world");
    }

    #[test]
    fn audio_append_is_bounded() {
        let mut buf = vec![0.0; MAX_BUFFER_SAMPLES - 1];
        assert!(append_audio_bounded(&mut buf, &[1.0, 2.0]));
        assert_eq!(buf.len(), MAX_BUFFER_SAMPLES);
        assert_eq!(buf[MAX_BUFFER_SAMPLES - 1], 1.0);
    }
}
