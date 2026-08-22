//! Synchronization primitives (`spinlock.h`; `sleeplock.c` later).
//!
//! `SpinLock` is the only primitive this far: mutual exclusion with
//! xv6's interrupt discipline.

mod spin;

pub use spin::{pop_off, push_off, SpinLock};
