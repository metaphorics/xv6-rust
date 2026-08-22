//! Per-hart bookkeeping (`struct cpu`, `proc.h:22-28`).
//!
//! Fields beyond the M2 lock-discipline pair (`noff`, `intena`):
//! `current` (`c->proc`, proc.h:23) and the scheduler's saved `context`
//! (proc.h:24). The cells are plain atomics / an `UnsafeCell` rather
//! than `Cell` types because a `static` must be `Sync`; nothing here is
//! ever shared across harts in practice — each hart touches only its own
//! row, with interrupts disabled, the `mycpu()` discipline of
//! proc.c:61-79. The atomics (all `Relaxed`) compile to plain loads and
//! stores and exist only to satisfy the type system under that
//! single-writer invariant.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use crate::arch;
use crate::arch::Context;
use crate::params::NCPU;

/// Per-hart state, one row per possible hart (`cpus[NCPU]`, `proc.h:29`).
pub struct Cpu {
    /// Depth of the `push_off` nesting: how many locks this hart holds
    /// with interrupts forced off (`noff`, `proc.h:24`).
    noff: AtomicI32,
    /// Whether interrupts were enabled when the outermost `push_off`
    /// happened, restored by the matching `pop_off` (`intena`,
    /// `proc.h:23`).
    intena: AtomicBool,
    /// The process running on this hart, as a proc-table slot; 0 = none
    /// (`c->proc`, proc.h:23). Stored slot+1 so 0 can mean "no process"
    /// without a sentinel enum.
    current: AtomicUsize,
    /// The scheduler's saved context; `swtch` target when a process
    /// gives up the hart (`c->context`, proc.h:24).
    context: UnsafeCell<Context>,
}

// SAFETY: each `Cpu` row is touched only by its own hart, with
// interrupts disabled (the mycpu() discipline, proc.c:61-79), so no two
// harts ever access the same cells concurrently; the `UnsafeCell`'s
// `Context` is only ever reached from `swtch` under that same
// discipline.
unsafe impl Sync for Cpu {}

const CPU_INIT: Cpu = Cpu {
    noff: AtomicI32::new(0),
    intena: AtomicBool::new(false),
    current: AtomicUsize::new(0),
    context: UnsafeCell::new(Context::ZERO),
};

static CPUS: [Cpu; NCPU] = [CPU_INIT; NCPU];

/// The calling hart's row. Must run with interrupts disabled: otherwise a
/// timer interrupt between reading `cpu_id()` and using the result could
/// migrate the caller, exactly the hazard `mycpu()` documents
/// (`proc.c:65-80`). Indexing panics rather than indexing out of bounds
/// if `tp` is corrupt.
pub fn current() -> &'static Cpu {
    &CPUS[arch::cpu_id()]
}

impl Cpu {
    /// Lock-nesting depth (spinlock.c uses `mycpu()->noff`).
    pub(crate) fn noff(&self) -> i32 {
        self.noff.load(Ordering::Relaxed)
    }

    pub(crate) fn set_noff(&self, depth: i32) {
        self.noff.store(depth, Ordering::Relaxed);
    }

    /// Saved interrupt state for the outermost `push_off`.
    pub(crate) fn intena(&self) -> bool {
        self.intena.load(Ordering::Relaxed)
    }

    pub(crate) fn set_intena(&self, enabled: bool) {
        self.intena.store(enabled, Ordering::Relaxed);
    }

    /// The process running on this hart, if any (`c->proc`).
    pub fn current_slot(&self) -> Option<usize> {
        let slot = self.current.load(Ordering::Relaxed);
        (slot != 0).then_some(slot - 1)
    }

    /// Set or clear the running process (`c->proc = p`, proc.c:434/452).
    pub fn set_current_slot(&self, slot: Option<usize>) {
        self.current.store(slot.map_or(0, |s| s + 1), Ordering::Relaxed);
    }

    /// The scheduler context's address, as `swtch` wants it
    /// (`&c->context`, proc.c:447). Reaching the cell this way is safe;
    /// actually context-switching through it is the caller's contract.
    pub fn scheduler_context(&self) -> *mut Context {
        self.context.get()
    }
}
