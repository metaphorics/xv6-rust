//! Spin lock (`kernel/spinlock.c`).
//!
//! The xv6 semantics preserved exactly:
//!
//! - `lock` disables interrupts via `push_off` *before* contending
//!   (spinlock.c:24), panics if this hart already holds the lock
//!   (spinlock.c:25-26), spins on an acquire swap (spinlock.c:37), and
//!   records the owner (spinlock.c:41).
//! - release (`SpinGuard::drop`) panics if not held (spinlock.c:48-49),
//!   clears the owner, stores the flag with release ordering
//!   (spinlock.c:51-73), and only then re-enables interrupts via
//!   `pop_off` (spinlock.c:75).

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::arch;
use crate::cpu;

/// `owner` value marking an unowned lock. Any other value is the owning
/// hart's id — the `lk->cpu` field of `struct spinlock` (`spinlock.h`),
/// with `usize::MAX` instead of C's null so the whole lock stays plain
/// safe atomics.
const UNOWNED: usize = usize::MAX;

/// A spin lock protecting `T`.
pub struct SpinLock<T> {
    locked: AtomicBool,
    owner: AtomicUsize,
    data: UnsafeCell<T>,
}

// SAFETY: moving a `SpinLock<T>` to another hart is sound whenever `T`
// itself can move between harts (`T: Send`); access to the data is
// mediated exclusively by the lock word.
unsafe impl<T: Send> Send for SpinLock<T> {}

// SAFETY: sharing `&SpinLock<T>` across harts hands out no access to
// `data` until a hart wins `locked`; `T: Send` is sufficient for that
// mediated access to be sound. (The core of every safe mutex.)
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    /// Create an unlocked lock; `const` so it can initialize `static`s
    /// (`initlock`, spinlock.c:11-17).
    pub const fn new(data: T) -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
            owner: AtomicUsize::new(UNOWNED),
            data: UnsafeCell::new(data),
        }
    }

    /// Acquire the lock, returning a guard that releases it on drop
    /// (`acquire`, spinlock.c:22-43).
    pub fn lock(&self) -> SpinGuard<'_, T> {
        // disable interrupts to avoid deadlock (spinlock.c:24).
        push_off();
        if self.holding() {
            panic!("acquire");
        }

        // `__sync_lock_test_and_set` with acquire ordering: the swap is
        // the lock, and no critical-section access may be reordered
        // before it (spinlock.c:28-38).
        while self.locked.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }

        // record the owner for `holding()` (spinlock.c:41).
        self.owner.store(arch::cpu_id(), Ordering::Relaxed);
        SpinGuard { lock: self }
    }

    /// Is the calling hart holding this lock? Interrupts must be off
    /// (`holding`, spinlock.c:78-86). C checks `locked && cpu==mycpu()`;
    /// the owner sentinel is equivalent: `owner == self` is observable
    /// only between `lock()`'s owner store and `release()`'s clear on
    /// this hart, in program order.
    pub fn holding(&self) -> bool {
        self.owner.load(Ordering::Relaxed) == arch::cpu_id()
    }

    /// Free the lock and restore the interrupt state (`release`,
    /// spinlock.c:46-76).
    fn release(&self) {
        if !self.holding() {
            panic!("release");
        }

        self.owner.store(UNOWNED, Ordering::Relaxed);
        // release store, so every critical-section store is visible
        // before the lock word clears (spinlock.c:73).
        self.locked.store(false, Ordering::Release);
        pop_off();
    }

    /// This lock's address, usable as a sleep channel — the C kernel
    /// sleeps on the address of the lock-protected condition (e.g.
    /// `sleep(&ticks, &tickslock)`, trap.c/sysproc.c).
    pub fn chan(&self) -> usize {
        self as *const Self as usize
    }

    /// Release without going through a guard: the scheduler/forkret
    /// handshake points, where the lock must stay held across a
    /// context switch and the paired release happens in a different
    /// call frame (forkret releasing what scheduler acquired, and the
    /// post-`sched` releases inside proc). Panics unless this hart
    /// holds the lock, exactly like `release`.
    pub(crate) fn release_raw(&self) {
        self.release();
    }

    /// Run `f` on the protected data while this hart already holds the
    /// lock — the read after `sched()` returns into a flow whose guard
    /// was consumed before the switch (proc's sleep clears `chan` this
    /// way). Asserts ownership like `holding`.
    pub(crate) fn with_held<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        assert!(self.holding(), "with_held: lock not held");
        // SAFETY: this hart holds the lock (asserted above), so this is
        // the only live access — the same guarantee a guard provides.
        unsafe { f(&mut *self.data.get()) }
    }
}

/// An acquired `SpinLock`, giving `&mut` access to the protected data.
/// Dropping releases the lock.
#[must_use = "dropping the guard releases the lock immediately"]
pub struct SpinGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<'a, T> SpinGuard<'a, T> {
    /// Surrender this guard to `proc::sleep`'s release/re-acquire
    /// handoff: returns the lock it held without releasing it (sleep
    /// releases under the lost-wakeup ordering and re-acquires before
    /// returning a fresh guard).
    pub(crate) fn handoff(self) -> &'a SpinLock<T> {
        let lock = self.lock;
        core::mem::forget(self);
        lock
    }
}

impl<T> Deref for SpinGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: the guard exists only while this hart holds `lock`;
        // mutual exclusion makes this the only live reference.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as `deref`, plus `&mut self` — the exclusive borrow of
        // the guard carries exclusivity into the protected data.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.release();
    }
}

/// Disable interrupts, tracking nesting depth (`push_off`,
/// spinlock.c:92-103).
///
/// The reference C fuses the read-and-clear into one `csrrc`
/// (`rc_sstatus`, riscv.h:76-81); this port follows the split
/// `intr_get()` + `intr_off()` form the brief prescribes, which has the
/// same observable contract.
pub fn push_off() {
    let old = arch::intr_get();
    arch::intr_off();
    let c = cpu::current();
    if c.noff() == 0 {
        c.set_intena(old);
    }
    c.set_noff(c.noff() + 1);
}

/// Undo one `push_off`, re-enabling interrupts only when the last lock is
/// released and they were enabled on entry (`pop_off`, spinlock.c:105-118).
pub fn pop_off() {
    let c = cpu::current();
    if arch::intr_get() {
        panic!("pop_off - interruptible");
    }
    if c.noff() < 1 {
        panic!("pop_off");
    }
    c.set_noff(c.noff() - 1);
    if c.noff() == 0 && c.intena() {
        arch::intr_on();
    }
}
