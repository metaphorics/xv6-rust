//! Synchronization primitives (`spinlock.h`, `sleeplock.h`).
//!
//! `SpinLock` (mutual exclusion with xv6's interrupt discipline) and
//! `SleepLock` (sleeping waiters over the sleep/wakeup channel
//! protocol).

mod sleeplock;
mod spin;

pub use sleeplock::SleepLock;
pub use spin::{pop_off, push_off, SpinGuard, SpinLock};
