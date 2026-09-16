//! Pollable cross-thread delivery for daemon events.
//!
//! `std::sync::mpsc` transports the typed payload, while an `eventfd`
//! makes every successful send visible to the main reactor. The eventfd
//! counter coalesces bursts without adding a periodic poll timeout.

use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::{Arc, mpsc};

use nix::errno::Errno;
use nix::sys::eventfd::{EfdFlags, EventFd};

use super::DaemonEvent;

/// Pollable wakeup shared by every daemon-event sender.
#[derive(Clone, Debug)]
pub(super) struct ReactorWaker {
    event_fd: Arc<EventFd>,
}

impl ReactorWaker {
    fn new() -> io::Result<Self> {
        let event_fd =
            EventFd::from_value_and_flags(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK)
                .map_err(errno_to_io)?;
        Ok(Self {
            event_fd: Arc::new(event_fd),
        })
    }

    pub(super) fn fd(&self) -> RawFd {
        self.event_fd.as_raw_fd()
    }

    /// Mark the reactor wake source readable.
    ///
    /// `EAGAIN` means the eventfd counter is saturated and therefore already
    /// readable, so it is equivalent to a successful wakeup.
    pub(super) fn wake(&self) -> io::Result<()> {
        loop {
            match self.event_fd.write(1) {
                Ok(_) => return Ok(()),
                Err(Errno::EINTR) => continue,
                Err(Errno::EAGAIN) => return Ok(()),
                Err(error) => return Err(errno_to_io(error)),
            }
        }
    }

    /// Clear all coalesced wakeups currently recorded by the eventfd.
    pub(super) fn drain(&self) -> io::Result<()> {
        loop {
            match self.event_fd.read() {
                Ok(_) | Err(Errno::EAGAIN) => return Ok(()),
                Err(Errno::EINTR) => continue,
                Err(error) => return Err(errno_to_io(error)),
            }
        }
    }
}

/// Sender that preserves the typed mpsc channel and wakes the reactor.
#[derive(Clone, Debug)]
pub(super) struct DaemonEventSender {
    sender: mpsc::Sender<DaemonEvent>,
    waker: ReactorWaker,
}

impl DaemonEventSender {
    pub(super) fn send(&self, event: DaemonEvent) -> Result<(), mpsc::SendError<DaemonEvent>> {
        self.sender.send(event)?;
        if let Err(error) = self.waker.wake() {
            tracing::error!(
                target: "typio.lifecycle",
                %error,
                "failed to wake daemon reactor after queuing event"
            );
        }
        Ok(())
    }
}

pub(super) fn channel() -> io::Result<(DaemonEventSender, mpsc::Receiver<DaemonEvent>, ReactorWaker)>
{
    let waker = ReactorWaker::new()?;
    let (sender, receiver) = mpsc::channel();
    Ok((
        DaemonEventSender {
            sender,
            waker: waker.clone(),
        },
        receiver,
        waker,
    ))
}

fn errno_to_io(error: Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll_readable(fd: RawFd, timeout_ms: i32) -> bool {
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        assert!(ready >= 0, "poll failed: {}", io::Error::last_os_error());
        ready == 1 && pollfd.revents & libc::POLLIN != 0
    }

    #[test]
    fn cross_thread_send_wakes_and_delivers() {
        let (sender, receiver, waker) = channel().unwrap();
        let worker = std::thread::spawn(move || sender.send(DaemonEvent::Shutdown));

        assert!(poll_readable(waker.fd(), 1_000));
        worker.join().unwrap().unwrap();
        waker.drain().unwrap();
        assert_eq!(receiver.try_recv(), Ok(DaemonEvent::Shutdown));
        assert!(!poll_readable(waker.fd(), 0));
    }

    #[test]
    fn wakeups_coalesce_and_drain_in_one_read() {
        let (_, _, waker) = channel().unwrap();
        waker.wake().unwrap();
        waker.wake().unwrap();
        waker.wake().unwrap();

        assert!(poll_readable(waker.fd(), 0));
        waker.drain().unwrap();
        assert!(!poll_readable(waker.fd(), 0));
    }
}
