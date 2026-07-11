//! Named poll sources and deadline reduction for the Wayland runtime loop.
//!
//! The runtime has a small, fixed set of file descriptors, so `poll(2)` is
//! sufficient. This module removes positional `fds[n]` coupling and keeps the
//! `-1`-means-unbounded timeout rule in one tested place.

use std::io;

/// Stable identity of every file descriptor watched by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub(super) enum PollSource {
    Wayland = 0,
    Uds = 1,
    KeyRepeat = 2,
    ConfigWatch = 3,
    ConfigTimer = 4,
    IndicatorTimer = 5,
    VoiceTimer = 6,
    VoiceSession = 7,
}

const SOURCE_COUNT: usize = 8;

/// Named descriptor snapshot used to construct the fixed poll set.
pub(super) struct PollSourceFds {
    pub wayland: i32,
    pub uds: i32,
    pub key_repeat: i32,
    pub config_watch: i32,
    pub config_timer: i32,
    pub indicator_timer: i32,
    pub voice_timer: i32,
    pub voice_session: i32,
}

/// Readiness snapshot returned by one `poll(2)` call.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReadySet {
    revents: [i16; SOURCE_COUNT],
}

impl ReadySet {
    pub fn readable(self, source: PollSource) -> bool {
        self.revents[source as usize] & libc::POLLIN != 0
    }

    pub fn disconnected(self, source: PollSource) -> bool {
        self.revents[source as usize] & (libc::POLLERR | libc::POLLHUP) != 0
    }
}

/// Fixed poll set with named source access.
pub(super) struct PollSources {
    fds: [libc::pollfd; SOURCE_COUNT],
}

impl PollSources {
    pub fn new(fds: PollSourceFds) -> Self {
        Self {
            fds: [
                pollfd(fds.wayland),
                pollfd(fds.uds),
                pollfd(fds.key_repeat),
                pollfd(fds.config_watch),
                pollfd(fds.config_timer),
                pollfd(fds.indicator_timer),
                pollfd(fds.voice_timer),
                pollfd(fds.voice_session),
            ],
        }
    }

    pub fn wait(&mut self, timeout: PollTimeout) -> io::Result<ReadySet> {
        for fd in &mut self.fds {
            fd.revents = 0;
        }
        let rc = unsafe {
            libc::poll(
                self.fds.as_mut_ptr(),
                self.fds.len() as libc::nfds_t,
                timeout.as_millis(),
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ReadySet {
            revents: self.fds.map(|fd| fd.revents),
        })
    }
}

fn pollfd(fd: i32) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

/// `poll(2)` timeout reducer. No deadline means an idle, unbounded wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PollTimeout(Option<i32>);

impl PollTimeout {
    pub fn reduce(&mut self, remaining_ms: i32) {
        let remaining_ms = remaining_ms.max(0);
        self.0 = Some(match self.0 {
            Some(current) => current.min(remaining_ms),
            None => remaining_ms,
        });
    }

    pub fn as_millis(self) -> i32 {
        self.0.unwrap_or(-1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_is_unbounded_without_deadlines() {
        assert_eq!(PollTimeout::default().as_millis(), -1);
    }

    #[test]
    fn timeout_reducer_keeps_earliest_nonnegative_deadline() {
        let mut timeout = PollTimeout::default();
        timeout.reduce(20);
        timeout.reduce(5);
        timeout.reduce(12);
        assert_eq!(timeout.as_millis(), 5);

        timeout.reduce(-4);
        assert_eq!(timeout.as_millis(), 0);
    }

    #[test]
    fn ready_set_uses_named_sources() {
        let mut revents = [0; SOURCE_COUNT];
        revents[PollSource::VoiceSession as usize] = libc::POLLIN;
        revents[PollSource::Wayland as usize] = libc::POLLHUP;
        let ready = ReadySet { revents };

        assert!(ready.readable(PollSource::VoiceSession));
        assert!(!ready.readable(PollSource::ConfigTimer));
        assert!(ready.disconnected(PollSource::Wayland));
    }
}
