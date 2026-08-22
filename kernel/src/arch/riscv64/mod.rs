//! riscv64 adapter for the QEMU `virt` machine.

pub mod entry;
pub mod start;
pub mod vm;

use core::arch::asm;

// sstatus.SIE: supervisor-mode interrupt enable (riscv.h:47).
const SSTATUS_SIE: usize = 1 << 1;

/// This hart's id, read from `tp` (`r_tp`, riscv.h:340-345; `cpuid`,
/// proc.c:65-70). `start` parks each hart's mhartid in `tp` before
/// `mret` (start.c:47).
pub fn cpu_id() -> usize {
    let id;
    // SAFETY: reading a register into a local; no memory is touched.
    unsafe { asm!("mv {id}, tp", id = out(reg) id, options(nomem, nostack)) };
    id
}

/// Enable device interrupts (`intr_on`, riscv.h:309-313).
pub fn intr_on() {
    // SAFETY: `csrs` sets only the SIE bit of sstatus; no memory effect.
    unsafe { asm!("csrs sstatus, {sie}", sie = in(reg) SSTATUS_SIE, options(nomem, nostack)) };
}

/// Disable device interrupts (`intr_off`, riscv.h:315-320).
pub fn intr_off() {
    // SAFETY: `csrc` clears only the SIE bit of sstatus; no memory
    // effect.
    unsafe { asm!("csrc sstatus, {sie}", sie = in(reg) SSTATUS_SIE, options(nomem, nostack)) };
}

/// Are device interrupts enabled? (`intr_get`, riscv.h:322-328.)
pub fn intr_get() -> bool {
    let sstatus: usize;
    // SAFETY: reading a CSR into a local; no memory is touched.
    unsafe { asm!("csrr {sstatus}, sstatus", sstatus = out(reg) sstatus, options(nomem, nostack)) };
    sstatus & SSTATUS_SIE != 0
}
