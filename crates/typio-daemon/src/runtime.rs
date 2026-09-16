//! Shared ownership for the single-threaded daemon runtime.

use std::cell::RefCell;
use std::rc::Rc;

/// Main-loop-owned Typio runtime shared by synchronous host subsystems.
///
/// Cross-thread sources must send a typed daemon event instead of accessing
/// this handle. That keeps registry and configuration mutation on the reactor
/// thread and makes the ownership boundary explicit.
pub type SharedInstance = Rc<RefCell<Box<typio_runtime::TypioInstance>>>;
