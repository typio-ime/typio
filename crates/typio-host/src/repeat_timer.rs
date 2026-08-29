//! Keyboard auto-repeat timer — pure mechanism.
//!
//! Owns the Linux timerfd that drives key auto-repeat plus the
//! compositor `repeat_info` parameter resolution. The modifier bitmask and
//! the "don't repeat while Ctrl/Alt/Super is held" gate live in
//! `typio_host_types::modifiers` so the platform layer can share one bit
//! layout; the dispatch state machine lives in `keyboard::router`.

use std::io;
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::time::Duration;

use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{ClockId, Expiration, TimerFd, TimerFlags, TimerSetTimeFlags};

/// Default initial delay before auto-repeat begins (matches X server
/// default; used until the compositor sends a `repeat_info` event).
pub const DEFAULT_DELAY: Duration = Duration::from_millis(600);

/// Default repeat rate in keys/sec, matching the X server default.
pub const DEFAULT_RATE: u32 = 30;

/// Resolve the auto-repeat delay and interval from the compositor's
/// `repeat_info` event (if any).
///
/// The input-method-v2 `repeat_info` event carries `(rate, delay)` where
/// `rate` is keys/sec and `delay` is milliseconds. A `rate` of `0` is the
/// protocol signal for "do not repeat"; this function returns `None` in
/// that case so the caller can skip arming the timer entirely. When the
/// compositor has not sent any `repeat_info` (`info == None`) the X-server
/// defaults ([`DEFAULT_DELAY`] + [`DEFAULT_RATE`]) are used.
pub fn resolve_repeat_params(compositor_info: Option<(i32, i32)>) -> Option<(Duration, Duration)> {
    match compositor_info {
        Some((rate, delay)) if rate > 0 => {
            let d = Duration::from_millis(delay.max(0) as u64);
            let i = RepeatTimer::interval_from_rate(rate as u32);
            Some((d, i))
        }
        Some(_) => None,
        None => Some((DEFAULT_DELAY, RepeatTimer::interval_from_rate(DEFAULT_RATE))),
    }
}

/// A keyboard-repeat timer.
///
/// Owns a Linux timerfd configured with an initial delay followed by a
/// recurring interval (the rate derived from `repeat_rate` in keys/sec).
/// Exposes the timer fd for integration with any external event loop.
pub struct RepeatTimer {
    timer: TimerFd,
    /// Last successfully requested kernel state, exposed for diagnostics.
    armed: bool,
}

impl RepeatTimer {
    /// Construct a disarmed timer.
    pub fn new() -> io::Result<Self> {
        let timer =
            TimerFd::new(ClockId::CLOCK_MONOTONIC, TimerFlags::TFD_NONBLOCK).map_err(nix_to_io)?;
        Ok(Self {
            timer,
            armed: false,
        })
    }

    /// The timer file descriptor. Add to your event loop with read interest.
    pub fn fd(&self) -> RawFd {
        self.timer.as_fd().as_raw_fd()
    }

    /// True iff the timer was last armed via [`Self::start`] and not
    /// subsequently stopped.
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Consume one readable timerfd expiration count.
    ///
    /// The reactor calls this only after `POLLIN`. Keeping the read beside the
    /// owned descriptor prevents input policy from handling raw timerfd bytes.
    pub fn consume_expiration(&self) -> io::Result<u64> {
        consume_timerfd(&self.timer)
    }

    /// Arm the timer with the given initial delay followed by a recurring
    /// `interval`. Subsequent dispatches fire once per `interval` until
    /// [`Self::stop`] is called.
    pub fn start(&mut self, delay: Duration, interval: Duration) -> io::Result<()> {
        let expiration = Expiration::IntervalDelayed(
            TimeSpec::from_duration(delay),
            TimeSpec::from_duration(interval),
        );
        self.timer
            .set(expiration, TimerSetTimeFlags::empty())
            .map_err(nix_to_io)?;
        self.armed = true;
        Ok(())
    }

    /// Disarm the timer. Safe to call on an already-disarmed timer;
    /// arming a timer with a zero `it_value` is the kernel-defined
    /// disarm semantic.
    pub fn stop(&mut self) -> io::Result<()> {
        // OneShot with zero duration disarms the timer (timerfd_settime(2)):
        // "Setting both fields of it_value to zero disarms the timer."
        let expiration = Expiration::OneShot(TimeSpec::from_duration(Duration::ZERO));
        self.timer
            .set(expiration, TimerSetTimeFlags::empty())
            .map_err(nix_to_io)?;
        self.armed = false;
        Ok(())
    }

    /// Compute the interval from a Wayland keyboard `repeat_rate`
    /// expressed in keys per second. Returns 1 ms minimum so a
    /// pathological high rate (e.g. 10000) does not produce a zero
    /// interval.
    pub fn interval_from_rate(repeat_rate: u32) -> Duration {
        if repeat_rate == 0 {
            // Caller should have checked; fall back to something sane
            // rather than dividing by zero.
            return Duration::from_millis(1000 / 30);
        }
        let ms = 1000 / repeat_rate;
        Duration::from_millis(ms.max(1) as u64)
    }
}

/// Consume the native `u64` expiration counter from any readable timerfd.
pub fn consume_timerfd(timer: &impl AsFd) -> io::Result<u64> {
    let mut bytes = [0u8; std::mem::size_of::<u64>()];
    let read = nix::unistd::read(timer, &mut bytes).map_err(nix_to_io)?;
    if read != bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short timerfd read",
        ));
    }
    Ok(u64::from_ne_bytes(bytes))
}

impl Default for RepeatTimer {
    fn default() -> Self {
        Self::new().expect("RepeatTimer::new should not fail under normal conditions")
    }
}

fn nix_to_io(e: nix::Error) -> io::Error {
    io::Error::from_raw_os_error(e as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_starts_disarmed() {
        let t = RepeatTimer::new().unwrap();
        assert!(!t.is_armed());
        assert!(t.fd() >= 0);
    }

    #[test]
    fn timer_arm_and_disarm_toggles_flag() {
        let mut t = RepeatTimer::new().unwrap();
        t.start(Duration::from_millis(50), Duration::from_millis(20))
            .unwrap();
        assert!(t.is_armed());
        t.stop().unwrap();
        assert!(!t.is_armed());
        // Stop on an already-disarmed timer is a no-op.
        t.stop().unwrap();
        assert!(!t.is_armed());
    }

    #[test]
    fn interval_from_rate_clamps_below_one_ms() {
        // 30 Hz → ~33ms
        assert_eq!(
            RepeatTimer::interval_from_rate(30),
            Duration::from_millis(33)
        );
        // 1 Hz → 1000ms
        assert_eq!(
            RepeatTimer::interval_from_rate(1),
            Duration::from_millis(1000)
        );
        // 10000 Hz → clamps to 1ms minimum
        assert_eq!(
            RepeatTimer::interval_from_rate(10000),
            Duration::from_millis(1)
        );
        // 0 (caller bug) → falls back to a sane default rather than panicking
        assert_eq!(
            RepeatTimer::interval_from_rate(0),
            Duration::from_millis(1000 / 30)
        );
    }

    #[test]
    fn resolve_repeat_params_defaults_when_no_compositor_info() {
        let (delay, interval) = resolve_repeat_params(None).expect("default params");
        assert_eq!(delay, DEFAULT_DELAY);
        assert_eq!(interval, RepeatTimer::interval_from_rate(DEFAULT_RATE));
    }

    #[test]
    fn resolve_repeat_params_uses_compositor_info_when_present() {
        let (delay, interval) =
            resolve_repeat_params(Some((25, 500))).expect("compositor-provided params");
        assert_eq!(delay, Duration::from_millis(500));
        assert_eq!(interval, RepeatTimer::interval_from_rate(25));
    }

    #[test]
    fn resolve_repeat_params_returns_none_when_rate_is_zero() {
        // rate == 0 is the protocol signal for "do not repeat".
        assert!(resolve_repeat_params(Some((0, 500))).is_none());
    }

    #[test]
    fn resolve_repeat_params_clamps_negative_delay() {
        let (delay, _) =
            resolve_repeat_params(Some((30, -10))).expect("clamped-negative-delay params");
        assert_eq!(delay, Duration::ZERO);
    }
}
