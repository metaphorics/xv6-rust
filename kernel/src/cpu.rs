//! Per-hart bookkeeping (`struct cpu`, `proc.h:22-28`) — for now the
//! fields the spinlock interrupt discipline needs (`noff`, `intena`,
//! `spinlock.c:88-118`).
//!
//! The cells are `Atomic*` rather than `Cell`: a `static` must be `Sync`,
//! and `Cell` is not. Nothing here is ever shared across harts in
//! practice — each hart touches only its own row, with interrupts
//! disabled, the `mycpu()` discipline of `proc.c:76-80`. The atomics
//! (all `Relaxed`) compile to plain loads and stores and exist only to
//! satisfy the type system under that single-writer invariant.

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::arch;
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
}

const CPU_INIT: Cpu = Cpu {
    noff: AtomicI32::new(0),
    intena: AtomicBool::new(false),
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
}
