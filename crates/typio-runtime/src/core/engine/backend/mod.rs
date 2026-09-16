//! Out-of-process engine backend support.
//!
//! The registry owns concrete [`process::ProcessBackend`] values. The old
//! multi-transport abstraction was removed; engines run as worker processes.

pub mod engine_protocol;
pub mod process;

pub use process::ProcessBackend;
