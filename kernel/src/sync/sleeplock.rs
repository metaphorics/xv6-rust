//! Sleep lock (`kernel/sleeplock.c`).
//!
//! A lock whose waiters sleep instead of spinning, for long critical
//! sections (the uart transmitter). Built from a spin lock guarding a
//! single `locked` word plus the sleep/wakeup channel protocol
//! (sleeplock.c:13-57).

use crate::proc;
use crate::sync::SpinLock;

/// A sleep lock (`struct sleeplock`, sleeplock.h:2-10). The waiter count
/// avoids scanning the process table on an uncontended release.
pub struct SleepLock {
    inner: SpinLock<SleepState>,
}

struct SleepState {
    locked: bool,
    waiters: usize,
}

impl SleepLock {
    /// An unlocked sleep lock (`initsleeplock`, sleeplock.c:15-20). The
    /// const construction replaces the init call.
    pub const fn new() -> Self {
        SleepLock {
            inner: SpinLock::new(SleepState {
                locked: false,
                waiters: 0,
            }),
        }
    }

    /// Acquire the lock, sleeping until it is free (`acquiresleep`,
    /// sleeplock.c:25-41). Must not be called from interrupt context —
    /// the waiter sleeps.
    pub fn acquire(&self) {
        let mut guard = self.inner.lock();
        while guard.locked {
            guard.waiters += 1;
            guard = proc::sleep(self.inner.chan(), guard);
            guard.waiters -= 1;
        }
        guard.locked = true;
    }

    /// Release the lock and wake one waiter (`releasesleep`,
    /// sleeplock.c:44-51).
    pub fn release(&self) {
        let mut guard = self.inner.lock();
        assert!(guard.locked, "releasesleep");
        guard.locked = false;
        let contested = guard.waiters != 0;
        drop(guard);
        if contested {
            proc::wakeup(self.inner.chan());
        }
    }
}
