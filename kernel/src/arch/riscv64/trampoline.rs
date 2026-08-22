//! Trampoline between user and kernel space (`kernel/trampoline.S`).
//!
//! The kernel maps this page at the same virtual address (`TRAMPOLINE`,
//! the highest user virtual address) in the kernel page table and in
//! every user page table, so it keeps executing correctly across the
//! `satp` switch. `kernel.ld` captures the `.trampsec` section at a page
//! boundary. `uservec` is the trap vector for user mode; `userret` is
//! the return path, entered either at its label (from
//! [`user_ret`]) or as the return address of `uservec`'s `jalr` into
//! `usertrap` (trampoline.S:98-101).

use crate::arch::riscv64::vm::{TRAMPOLINE, TRAPFRAME};

unsafe extern "C" {
    /// First byte of the trampoline page (`trampoline`, trampoline.S:19).
    static trampoline: u8;
    /// User-mode trap vector (`uservec`, trampoline.S:22).
    static uservec: u8;
    /// Kernel-to-user return path (`userret`, trampoline.S:101).
    static userret: u8;
}

core::arch::global_asm!(
    // The C file's bare `.section trampsec` relies on gas defaults; the
    // explicit flags keep LLVM's integrated assembler happy while the
    // linker script still matches the section by name.
    ".section trampsec, \"ax\", @progbits",
    ".globl trampoline",
    "trampoline:",
    // The reference's `.align 4` (trampoline.S:20) is 16 bytes in gas.
    ".p2align 4",
    ".globl uservec",
    "uservec:",
    // trap.c sets stvec to point here, so traps from user space start
    // here, in supervisor mode, but with a user page table
    // (trampoline.S:23-28).
    //
    // save user a0 in sscratch so a0 can be used to get at TRAPFRAME
    // (trampoline.S:30-32).
    "    csrw    sscratch, a0",
    //
    // each process has a separate p->trapframe memory area, but it's
    // mapped to the same virtual address (TRAPFRAME) in every process's
    // user page table (trampoline.S:34-37).
    "    li      a0, {trapframe}",
    //
    // save the user registers in TRAPFRAME (trampoline.S:39-69).
    "    sd      ra, 40(a0)",
    "    sd      sp, 48(a0)",
    "    sd      gp, 56(a0)",
    "    sd      tp, 64(a0)",
    "    sd      t0, 72(a0)",
    "    sd      t1, 80(a0)",
    "    sd      t2, 88(a0)",
    "    sd      s0, 96(a0)",
    "    sd      s1, 104(a0)",
    "    sd      a1, 120(a0)",
    "    sd      a2, 128(a0)",
    "    sd      a3, 136(a0)",
    "    sd      a4, 144(a0)",
    "    sd      a5, 152(a0)",
    "    sd      a6, 160(a0)",
    "    sd      a7, 168(a0)",
    "    sd      s2, 176(a0)",
    "    sd      s3, 184(a0)",
    "    sd      s4, 192(a0)",
    "    sd      s5, 200(a0)",
    "    sd      s6, 208(a0)",
    "    sd      s7, 216(a0)",
    "    sd      s8, 224(a0)",
    "    sd      s9, 232(a0)",
    "    sd      s10, 240(a0)",
    "    sd      s11, 248(a0)",
    "    sd      t3, 256(a0)",
    "    sd      t4, 264(a0)",
    "    sd      t5, 272(a0)",
    "    sd      t6, 280(a0)",
    //
    // save the user a0 in p->trapframe->a0 (trampoline.S:71-73).
    "    csrr    t0, sscratch",
    "    sd      t0, 112(a0)",
    //
    // initialize kernel stack pointer, from p->trapframe->kernel_sp
    // (trampoline.S:75-76).
    "    ld      sp, 8(a0)",
    //
    // make tp hold the current hartid, from p->trapframe->kernel_hartid
    // (trampoline.S:78-79).
    "    ld      tp, 32(a0)",
    //
    // load the address of usertrap(), from p->trapframe->kernel_trap
    // (trampoline.S:81-82).
    "    ld      t0, 16(a0)",
    //
    // fetch the kernel page table address, from p->trapframe->kernel_satp
    // (trampoline.S:84-85).
    "    ld      t1, 0(a0)",
    //
    // wait for any previous memory operations to complete, so that they
    // use the user page table (trampoline.S:87-89).
    "    sfence.vma zero, zero",
    //
    // install the kernel page table (trampoline.S:91-92).
    "    csrw    satp, t1",
    //
    // flush now-stale user entries from the TLB (trampoline.S:94-95).
    "    sfence.vma zero, zero",
    //
    // call usertrap(); its return lands on the userret label just below,
    // with the user satp in a0 (trampoline.S:97-98).
    "    jalr    t0",
    //
    // usertrap() returns here, with user satp in a0; return from kernel
    // to user (trampoline.S:100-103).
    ".globl userret",
    "userret:",
    //
    // flush icache, in case this is the first time we're running this
    // proc on this hart (trampoline.S:105-107).
    "    fence.i",
    //
    // switch to the user page table (trampoline.S:109-112).
    "    sfence.vma zero, zero",
    "    csrw    satp, a0",
    "    sfence.vma zero, zero",
    //
    "    li      a0, {trapframe}",
    //
    // restore all but a0 from TRAPFRAME (trampoline.S:116-146).
    "    ld      ra, 40(a0)",
    "    ld      sp, 48(a0)",
    "    ld      gp, 56(a0)",
    "    ld      tp, 64(a0)",
    "    ld      t0, 72(a0)",
    "    ld      t1, 80(a0)",
    "    ld      t2, 88(a0)",
    "    ld      s0, 96(a0)",
    "    ld      s1, 104(a0)",
    "    ld      a1, 120(a0)",
    "    ld      a2, 128(a0)",
    "    ld      a3, 136(a0)",
    "    ld      a4, 144(a0)",
    "    ld      a5, 152(a0)",
    "    ld      a6, 160(a0)",
    "    ld      a7, 168(a0)",
    "    ld      s2, 176(a0)",
    "    ld      s3, 184(a0)",
    "    ld      s4, 192(a0)",
    "    ld      s5, 200(a0)",
    "    ld      s6, 208(a0)",
    "    ld      s7, 216(a0)",
    "    ld      s8, 224(a0)",
    "    ld      s9, 232(a0)",
    "    ld      s10, 240(a0)",
    "    ld      s11, 248(a0)",
    "    ld      t3, 256(a0)",
    "    ld      t4, 264(a0)",
    "    ld      t5, 272(a0)",
    "    ld      t6, 280(a0)",
    //
    // restore user a0 (trampoline.S:148-149).
    "    ld      a0, 112(a0)",
    //
    // return to user mode and user pc; usertrapret() sets up sstatus and
    // sepc (trampoline.S:151-153).
    "    sret",
    trapframe = const TRAPFRAME.0,
);

/// Physical (== kernel virtual) address of the trampoline page, for the
/// `TRAMPOLINE` mapping in the kernel and per-process page tables
/// (`(uint64)trampoline`, vm.c:46 / proc.c:189). Only the symbol's
/// location is used; the byte itself is the first instruction.
pub fn addr() -> usize {
    (&raw const trampoline) as usize
}

/// `stvec` value for taking traps from user mode:
/// `TRAMPOLINE + (uservec - trampoline)` (trap.c:107-108).
pub fn uservec_va() -> usize {
    // Address arithmetic on linker-defined symbols: only their locations
    // are meaningful, and both live in the same page.
    let base = (&raw const trampoline) as u64;
    let off = (&raw const uservec) as u64 - base;
    (TRAMPOLINE.0 + off) as usize
}

/// `userret`'s entry, computed the same way
/// (`TRAMPOLINE + (userret - trampoline)`, proc.c:542).
fn userret_va() -> usize {
    let base = (&raw const trampoline) as u64;
    let off = (&raw const userret) as u64 - base;
    (TRAMPOLINE.0 + off) as usize
}

/// Enter user mode through the trampoline: jump to `userret` at its
/// `TRAMPOLINE` alias with `satp` in `a0`, never to return
/// (`((void (*)(uint64))trampoline_userret)(satp)`, proc.c:543 — the
/// tail of `usertrapret` in upstream xv6). The caller must have run
/// the `usertrapret` register setup first.
pub fn user_ret(satp: u64) -> ! {
    let entry: unsafe extern "C" fn(u64) -> ! =
        // SAFETY: transmuting a code address that the linker placed at a
        // page-aligned, executable mapping (TRAMPOLINE) into a function
        // pointer of the trampoline's true signature (one argument in
        // a0, never returns: it `sret`s to user mode).
        unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(u64) -> !>(userret_va()) };
    // SAFETY: the trampoline contract — registers are prepared by
    // `usertrapret`, and `userret` never returns.
    unsafe { entry(satp) }
}
