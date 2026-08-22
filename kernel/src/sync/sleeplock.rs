//! Sleep lock (`kernel/sleeplock.c`).
//!
//! A lock whose waiters sleep instead of spinning, for long critical
//! sections (the uart transmitter). Built from a spin lock guarding a
//! single `locked` word plus the sleep/wakeup channel protocol
//! (sleeplock.c:13-57).



use crate::proc;
use crate::sync::SpinLock;

/// A sleep lock (`struct sleeplock`, sleeplock.h:2-10). The reference's
/// `pid` debug field is cut; nothing reads it.
pub struct SleepLock {
    /// The `locked` word and its spin lock (`lk` + `locked`,
    /// sleeplock.h:3-4). Sleeping on the spin lock's own address is the
    /// channel protocol (`sleep(&lk->locked, &lk->lk)`, sleeplock.c:33).
    inner: SpinLock<u32>,
}

impl SleepLock {
    /// An unlocked sleep lock (`initsleeplock`, sleeplock.c:15-20). The
    /// const construction replaces the init call.
    pub const fn new() -> Self {
        SleepLock {
            inner: SpinLock::new(0),
        }
    }

    /// Acquire the lock, sleeping until it is free (`acquiresleep`,
    /// sleeplock.c:25-41). Must not be called from interrupt context —
    /// the waiter sleeps.
    pub fn acquire(&self) {
        let mut guard = self.inner.lock();
        while *guard != 0 {
            guard = proc::sleep(self.inner.chan(), guard);
        }
        *guard = 1;
    }

    /// Release the lock and wake one waiter (`releasesleep`,
    /// sleeplock.c:44-51).
    pub fn release(&self) {
        let mut guard = self.inner.lock();
        *guard = 0;
        drop(guard);
        proc::wakeup(self.inner.chan());
    }
}
