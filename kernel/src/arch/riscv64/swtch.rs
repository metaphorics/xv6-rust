//! Kernel context switch (`kernel/swtch.S`).
//!
//! `swtch` saves the calling hart's callee-saved registers into the old
//! context and loads the new one's, then returns into the `ra` the new
//! context saved — switching kernel stacks as a side effect of restoring
//! `sp`. Caller-saved registers are the caller's problem per the usual
//! ABI, which is why the register set is exactly `ra`, `sp`, `s0`-`s11`.

/// Saved registers for kernel context switches (`struct context`,
/// proc.h:2-19).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Context {
    pub ra: u64,
    pub sp: u64,

    // callee-saved
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
}

/// The 14 quadwords of `struct context` (proc.h:2-19, swtch.S:10-40).
const _: () = assert!(core::mem::size_of::<Context>() == 112);

impl Context {
    /// A zeroed context; `allocproc` fills `ra`/`sp` over it
    /// (proc.c:145-147).
    pub const ZERO: Context = Context {
        ra: 0,
        sp: 0,
        s0: 0,
        s1: 0,
        s2: 0,
        s3: 0,
        s4: 0,
        s5: 0,
        s6: 0,
        s7: 0,
        s8: 0,
        s9: 0,
        s10: 0,
        s11: 0,
    };
    pub const fn new(entry: u64, stack_top: u64) -> Context {
        Context {
            ra: entry,
            sp: stack_top,
            ..Context::ZERO
        }
    }
}

core::arch::global_asm!(
    // void swtch(struct context *old, struct context *new);
    // Save current registers in old. Load from new. (swtch.S:3-5)
    ".globl swtch",
    "swtch:",
    "    sd      ra, 0(a0)",
    "    sd      sp, 8(a0)",
    "    sd      s0, 16(a0)",
    "    sd      s1, 24(a0)",
    "    sd      s2, 32(a0)",
    "    sd      s3, 40(a0)",
    "    sd      s4, 48(a0)",
    "    sd      s5, 56(a0)",
    "    sd      s6, 64(a0)",
    "    sd      s7, 72(a0)",
    "    sd      s8, 80(a0)",
    "    sd      s9, 88(a0)",
    "    sd      s10, 96(a0)",
    "    sd      s11, 104(a0)",
    "    ld      ra, 0(a1)",
    "    ld      sp, 8(a1)",
    "    ld      s0, 16(a1)",
    "    ld      s1, 24(a1)",
    "    ld      s2, 32(a1)",
    "    ld      s3, 40(a1)",
    "    ld      s4, 48(a1)",
    "    ld      s5, 56(a1)",
    "    ld      s6, 64(a1)",
    "    ld      s7, 72(a1)",
    "    ld      s8, 80(a1)",
    "    ld      s9, 88(a1)",
    "    ld      s10, 96(a1)",
    "    ld      s11, 104(a1)",
    "    ret",
);

unsafe extern "C" {
    /// The assembly above: save the callee-saved set into `old`, load it
    /// from `new` (`swtch`, swtch.S:9-40).
    fn swtch(old: *mut Context, new: *const Context);
}

/// Context-switch into `new`, saving the current register set in `old`
/// (`swtch(&c->context, &p->context)` and its inverse, proc.c:453/495).
///
/// # Safety
///
/// - `old` and `new` must point to distinct, live `Context`s that remain
///   valid across the call (one typically lives in a proc slot, the other
///   in this hart's `Cpu` row).
/// - Interrupts must be disabled: `swtch` leaves the hart's lock
///   nesting state (`noff`) as it found it, and a trap between the
///   register save and the restore would observe a half-switched thread.
pub unsafe fn switch(old: *mut Context, new: *const Context) {
    // SAFETY: caller guarantees the two pointers per the contract above;
    // the assembly touches no other memory.
    unsafe { swtch(old, new) }
}
