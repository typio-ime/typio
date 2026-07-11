//! One-shot timerfd with explicit armed state.
//!
//! The timerfd is itself a reactor source, so no duplicate user-space
//! deadline is needed to wake the loop.

use std::io;
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::time::Duration;

use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{ClockId, Expiration, TimerFd, TimerFlags, TimerSetTimeFlags};

use crate::repeat_timer::consume_timerfd;

pub(super) struct OneShotTimer {
    timer: TimerFd,
    armed: bool,
}

impl OneShotTimer {
    pub fn new() -> io::Result<Self> {
        let timer =
            TimerFd::new(ClockId::CLOCK_MONOTONIC, TimerFlags::TFD_NONBLOCK).map_err(nix_to_io)?;
        Ok(Self {
            timer,
            armed: false,
        })
    }

    pub fn fd(&self) -> RawFd {
        self.timer.as_fd().as_raw_fd()
    }

    pub fn arm(&mut self, duration: Duration) -> io::Result<()> {
        let expiration = Expiration::OneShot(TimeSpec::from_duration(duration));
        self.timer
            .set(expiration, TimerSetTimeFlags::empty())
            .map_err(nix_to_io)?;
        self.armed = true;
        Ok(())
    }

    pub fn disarm(&mut self) -> io::Result<()> {
        let expiration = Expiration::OneShot(TimeSpec::from_duration(Duration::ZERO));
        self.timer
            .set(expiration, TimerSetTimeFlags::empty())
            .map_err(nix_to_io)?;
        self.armed = false;
        Ok(())
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn consume_expiration(&mut self) -> io::Result<u64> {
        let expirations = consume_timerfd(&self.timer)?;
        self.armed = false;
        Ok(expirations)
    }
}

fn nix_to_io(error: nix::Error) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_and_disarm_track_kernel_state() {
        let mut timer = OneShotTimer::new().unwrap();
        assert!(!timer.is_armed());

        timer.arm(Duration::from_millis(50)).unwrap();
        assert!(timer.is_armed());

        timer.disarm().unwrap();
        assert!(!timer.is_armed());
    }
}
