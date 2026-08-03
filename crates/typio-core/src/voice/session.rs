//! Rust-native voice session state machine.
//!
//! Audio capture is injected through [`AudioSource`]. The session owns its
//! buffer and inference lifecycle, while a process-engine handle is supplied
//! at recording start so no registry pointer crosses a thread boundary.

use crate::core::engine::backend::process::VoiceProcessHandle;
use crate::voice::audio::{INITIAL_BUFFER_SAMPLES, MAX_BUFFER_SAMPLES, prepare_audio};
use crate::voice::types::{VoiceEvent, VoiceState};
use nix::sys::eventfd::{EfdFlags, EventFd};
use std::collections::VecDeque;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

/// Frontend-provided microphone capture.
pub trait AudioSource: Send {
    /// Start capture and push decoded 16 kHz mono samples into `sink`.
    fn start(&mut self, sink: AudioSink) -> bool;
    /// Stop capture and wait until no producer can use the sink any longer.
    fn stop(&mut self);
}

/// Cloneable, thread-safe destination handed to an [`AudioSource`].
#[derive(Clone)]
pub struct AudioSink {
    session: Weak<VoiceSession>,
}

impl AudioSink {
    /// Append samples while the session is recording.
    pub fn feed(&self, samples: &[f32]) {
        if let Some(session) = self.session.upgrade() {
            session.feed_audio(samples);
        }
    }
}

/// Shared voice recording and inference state.
pub struct VoiceSession {
    source: Mutex<Box<dyn AudioSource>>,
    state: Mutex<VoiceState>,
    audio_truncated: AtomicBool,
    audio_buffer: Mutex<Vec<f32>>,
    inference_target: Mutex<Option<VoiceProcessHandle>>,
    infer_handle: Mutex<Option<thread::JoinHandle<Option<String>>>>,
    event_fd: Arc<EventFd>,
    events: Mutex<VecDeque<VoiceEvent>>,
}

impl VoiceSession {
    /// Create a pollable session around a frontend audio source.
    pub fn new(source: Box<dyn AudioSource>) -> std::io::Result<Arc<Self>> {
        let event_fd =
            EventFd::from_value_and_flags(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK)
                .map_err(std::io::Error::from)?;
        Ok(Arc::new(Self {
            source: Mutex::new(source),
            state: Mutex::new(VoiceState::Idle),
            audio_truncated: AtomicBool::new(false),
            audio_buffer: Mutex::new(Vec::with_capacity(INITIAL_BUFFER_SAMPLES)),
            inference_target: Mutex::new(None),
            infer_handle: Mutex::new(None),
            event_fd: Arc::new(event_fd),
            events: Mutex::new(VecDeque::new()),
        }))
    }

    /// Begin capture using an owned snapshot of the active voice worker.
    pub fn start(self: &Arc<Self>, target: VoiceProcessHandle) -> bool {
        let mut state = self.state.lock().unwrap();
        if *state != VoiceState::Idle {
            return false;
        }
        self.audio_buffer.lock().unwrap().clear();
        self.audio_truncated.store(false, Ordering::Release);
        *self.inference_target.lock().unwrap() = Some(target);
        *state = VoiceState::Recording;

        let sink = AudioSink {
            session: Arc::downgrade(self),
        };
        if !self.source.lock().unwrap().start(sink) {
            *self.inference_target.lock().unwrap() = None;
            *state = VoiceState::Idle;
            return false;
        }
        drop(state);
        self.push_event(VoiceEvent::StateChange(VoiceState::Recording));
        true
    }

    /// Stop capture and start inference on a worker thread.
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        if *state != VoiceState::Recording {
            return;
        }
        *state = VoiceState::Processing;
        drop(state);

        self.source.lock().unwrap().stop();
        let mut audio = std::mem::take(&mut *self.audio_buffer.lock().unwrap());
        let target = self.inference_target.lock().unwrap().take();
        let sample_count = audio.len();
        let truncated = self.audio_truncated.swap(false, Ordering::AcqRel);
        log::info!(
            "Voice recording stopped, starting inference ({} samples)",
            sample_count
        );
        if truncated {
            log::warn!("Voice recording exceeded 60 seconds; trailing audio was discarded");
        }
        self.push_event(VoiceEvent::StateChange(VoiceState::Processing));

        let event_fd = self.event_fd.clone();
        let handle = thread::spawn(move || {
            let audio = prepare_audio(&mut audio);
            let result = if audio.is_empty() {
                log::warn!("Voice inference: no audio captured or usable");
                None
            } else {
                target.and_then(|worker| {
                    log::info!("Voice inference: processing {} samples", audio.len());
                    worker.process_audio(&audio)
                })
            };
            let value: u64 = 1;
            unsafe {
                let _ = libc::write(
                    event_fd.as_raw_fd(),
                    &value as *const _ as *const libc::c_void,
                    std::mem::size_of::<u64>(),
                );
            }
            result
        });
        *self.infer_handle.lock().unwrap() = Some(handle);
    }

    /// Pollable eventfd which signals inference completion.
    pub fn fd(&self) -> i32 {
        self.event_fd.as_raw_fd()
    }

    /// Consume a completion signal, join inference, and enqueue owned events.
    pub fn dispatch(&self) {
        let mut value = 0u64;
        unsafe {
            let _ = libc::read(
                self.event_fd.as_raw_fd(),
                &mut value as *mut _ as *mut libc::c_void,
                std::mem::size_of::<u64>(),
            );
        }
        let result =
            self.infer_handle
                .lock()
                .unwrap()
                .take()
                .and_then(|handle| match handle.join() {
                    Ok(result) => result,
                    Err(_) => {
                        self.push_event(VoiceEvent::Error(
                            "voice inference thread panicked".into(),
                        ));
                        None
                    }
                });
        *self.state.lock().unwrap() = VoiceState::Idle;
        if let Some(text) = result {
            let filtered = filter_tags(&text);
            let trimmed = filtered.trim();
            if !trimmed.is_empty() {
                self.push_event(VoiceEvent::Result(trimmed.to_string()));
            }
        }
        self.push_event(VoiceEvent::StateChange(VoiceState::Idle));
    }

    /// Drain state, result, and error events in production order.
    pub fn drain_events(&self) -> Vec<VoiceEvent> {
        self.events.lock().unwrap().drain(..).collect()
    }

    fn push_event(&self, event: VoiceEvent) {
        self.events.lock().unwrap().push_back(event);
    }

    fn feed_audio(&self, samples: &[f32]) {
        if samples.is_empty() || *self.state.lock().unwrap() != VoiceState::Recording {
            return;
        }
        if append_audio_bounded(&mut self.audio_buffer.lock().unwrap(), samples) {
            self.audio_truncated.store(true, Ordering::Release);
        }
    }
}

impl Drop for VoiceSession {
    fn drop(&mut self) {
        self.source.get_mut().unwrap().stop();
        if let Some(handle) = self.infer_handle.get_mut().unwrap().take() {
            let _ = handle.join();
        }
    }
}

fn append_audio_bounded(buffer: &mut Vec<f32>, samples: &[f32]) -> bool {
    let remaining = MAX_BUFFER_SAMPLES.saturating_sub(buffer.len());
    let accepted = remaining.min(samples.len());
    buffer.extend_from_slice(&samples[..accepted]);
    accepted < samples.len()
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
        let mut buffer = vec![0.0; MAX_BUFFER_SAMPLES - 1];
        assert!(append_audio_bounded(&mut buffer, &[1.0, 2.0]));
        assert_eq!(buffer.len(), MAX_BUFFER_SAMPLES);
        assert_eq!(buffer[MAX_BUFFER_SAMPLES - 1], 1.0);
    }
}
