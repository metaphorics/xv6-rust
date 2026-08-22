//! Kernel-mode trap entry (`kernel/kernelvec.S`).
//!
//! Interrupts and exceptions while in supervisor mode arrive here on
//! whatever kernel stack is current: push the caller-saved registers,
//! call [`crate::trap::kerneltrap`], restore, `sret`.

use crate::trap::kerneltrap;

core::arch::global_asm!(
    // The reference's `.align 4` (kernelvec.S:11) is a power-of-two
    // directive in riscv gas: 16-byte alignment, beyond the 4 bytes
    // stvec's direct mode requires.
    ".section .text.kernelvec, \"ax\", @progbits",
    ".p2align 4",
    ".globl kernelvec",
    "kernelvec:",
    // make room to save registers (kernelvec.S:13-14).
    "    addi    sp, sp, -256",
    // save caller-saved registers (kernelvec.S:16-35). sp and tp are
    // deliberately skipped: sp is restored by the frame teardown below,
    // and tp holds the hart id ("not tp (contains hartid), in case we
    // moved CPUs", kernelvec.S:20,44).
    "    sd      ra, 0(sp)",
    "    sd      gp, 16(sp)",
    "    sd      t0, 32(sp)",
    "    sd      t1, 40(sp)",
    "    sd      t2, 48(sp)",
    "    sd      a0, 72(sp)",
    "    sd      a1, 80(sp)",
    "    sd      a2, 88(sp)",
    "    sd      a3, 96(sp)",
    "    sd      a4, 104(sp)",
    "    sd      a5, 112(sp)",
    "    sd      a6, 120(sp)",
    "    sd      a7, 128(sp)",
    "    sd      t3, 216(sp)",
    "    sd      t4, 224(sp)",
    "    sd      t5, 232(sp)",
    "    sd      t6, 240(sp)",
    // call the trap handler in trap.rs (kernelvec.S:37-38).
    "    call    {handler}",
    // restore registers (kernelvec.S:40-59).
    "    ld      ra, 0(sp)",
    "    ld      gp, 16(sp)",
    "    ld      t0, 32(sp)",
    "    ld      t1, 40(sp)",
    "    ld      t2, 48(sp)",
    "    ld      a0, 72(sp)",
    "    ld      a1, 80(sp)",
    "    ld      a2, 88(sp)",
    "    ld      a3, 96(sp)",
    "    ld      a4, 104(sp)",
    "    ld      a5, 112(sp)",
    "    ld      a6, 120(sp)",
    "    ld      a7, 128(sp)",
    "    ld      t3, 216(sp)",
    "    ld      t4, 224(sp)",
    "    ld      t5, 232(sp)",
    "    ld      t6, 240(sp)",
    "    addi    sp, sp, 256",
    // return to whatever we were doing in the kernel (kernelvec.S:61-63).
    "    sret",
    handler = sym kerneltrap,
);

unsafe extern "C" {
    /// First byte of the vector, defined by the assembly above
    /// (`kernelvec`, kernelvec.S:12).
    static kernelvec: u8;
}

/// Address of the kernel trap vector, for `stvec` (`trapinithart`,
/// trap.c:26-30). Only the symbol's location is used; the byte itself is
/// the vector's first instruction and is never read as data.
pub fn addr() -> usize {
    (&raw const kernelvec) as usize
}
