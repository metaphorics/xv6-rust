//! Machine-mode bring-up (`kernel/start.c`), entered by every hart from
//! `entry` on its boot stack.

use core::arch::asm;

use super::{TIMER_INTERVAL, r_time, w_stimecmp};
use crate::main;

// mstatus.MPP: previous privilege mode for mret (riscv.h:14-17).
const MSTATUS_MPP_MASK: usize = 3 << 11;
const MSTATUS_MPP_S: usize = 1 << 11;

// sie bits (riscv.h:99-100).
const SIE_SEIE: usize = 1 << 9; // supervisor external
const SIE_STIE: usize = 1 << 5; // supervisor timer

// menvcfg (riscv.h:213-214).
const MENVCFG_ADUE: usize = 1 << 61; // hardware A/D bit updates
const MENVCFG_STCE: usize = 1 << 63; // sstc extension (stimecmp)

// mcounteren.TM, bit 1: supervisor may read `time` (start.c:62).
const MCOUNTEREN_TM: usize = 1 << 1;

/// Read `mstatus`.
fn r_mstatus() -> usize {
    let v;
    // SAFETY: reading a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrr {v}, mstatus", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Write `mstatus`.
fn w_mstatus(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw mstatus, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Write `mepc`, the address `mret` returns to.
fn w_mepc(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw mepc, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Write `satp` (page-table base; 0 disables paging).
fn w_satp(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw satp, {v}", v = in(reg) v, options(nostack)) };
}

/// Write `medeleg`, the exception-delegation mask.
fn w_medeleg(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw medeleg, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Write `mideleg`, the interrupt-delegation mask.
fn w_mideleg(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw mideleg, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Read `sie`, the supervisor interrupt-enable register.
fn r_sie() -> usize {
    let v;
    // SAFETY: reading a supervisor CSR has no effect on memory or stack.
    unsafe { asm!("csrr {v}, sie", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Write `sie`.
fn w_sie(v: usize) {
    // SAFETY: writing a supervisor CSR has no effect on memory or stack.
    unsafe { asm!("csrw sie, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Write `pmpaddr0`.
fn w_pmpaddr0(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw pmpaddr0, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Write `pmpcfg0`.
fn w_pmpcfg0(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw pmpcfg0, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Read `menvcfg`.
fn r_menvcfg() -> usize {
    let v;
    // SAFETY: reading a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrr {v}, menvcfg", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Write `menvcfg`.
fn w_menvcfg(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw menvcfg, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Read `mcounteren`.
fn r_mcounteren() -> usize {
    let v;
    // SAFETY: reading a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrr {v}, mcounteren", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Write `mcounteren`.
fn w_mcounteren(v: usize) {
    // SAFETY: writing a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrw mcounteren, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Read `mhartid`, this hart's id.
fn r_mhartid() -> usize {
    let v;
    // SAFETY: reading a machine CSR has no effect on memory or stack.
    unsafe { asm!("csrr {v}, mhartid", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Ask each hart to generate timer interrupts (`timerinit`,
/// start.c:54-66): enable the sstc extension, let supervisor mode at
/// `time`/`stimecmp`, and arm the very first interrupt. Later rearms
/// happen in supervisor mode, at the tail of `clockintr` (trap.c:179).
fn timerinit() {
    // enable the sstc extension (i.e. stimecmp) (start.c:58-59).
    w_menvcfg(r_menvcfg() | MENVCFG_STCE);

    // allow supervisor to use stimecmp and time (start.c:61-62).
    w_mcounteren(r_mcounteren() | MCOUNTEREN_TM);

    // ask for the very first timer interrupt (start.c:64-65).
    w_stimecmp(r_time() + TIMER_INTERVAL);
}

/// Write `tp`, which the kernel reserves for this hart's id (`cpuid`).
fn w_tp(v: usize) {
    // SAFETY: moving a value into the thread-pointer register; the kernel
    // uses no thread-local storage, so repurposing tp as the hart id (as
    // `w_tp` in riscv.h:348-352 does) cannot break compiler-generated code.
    unsafe { asm!("mv tp, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Machine-mode bring-up, then `mret` into supervisor-mode `main`
/// (`start`, start.c:14-52).
pub extern "C" fn start() -> ! {
    // set M Previous Privilege mode to Supervisor, for mret (start.c:17-21).
    w_mstatus((r_mstatus() & !MSTATUS_MPP_MASK) | MSTATUS_MPP_S);

    // set M Exception Program Counter to main, for mret (start.c:23-25).
    // The medium code model (kernel/.cargo/config.toml) keeps `main`'s
    // address materializable, the counterpart of gcc -mcmodel=medany.
    w_mepc(main as *const () as usize);

    // disable paging for now (start.c:27-28).
    w_satp(0);

    // delegate all interrupts and exceptions to supervisor mode (start.c:30-33).
    w_medeleg(0xffff);
    w_mideleg(0xffff);
    w_sie(r_sie() | SIE_SEIE | SIE_STIE);

    // configure Physical Memory Protection to give supervisor mode
    // access to all of physical memory (start.c:35-38).
    w_pmpaddr0(0x3f_ffff_ffff_ffff);
    w_pmpcfg0(0xf);

    // enable hardware updates of page table A and D bits (start.c:40-41).
    w_menvcfg(r_menvcfg() | MENVCFG_ADUE);

    // ask for clock interrupts (start.c:43-44).
    timerinit();

    // keep each hart's hartid in its tp register, for cpuid() (start.c:46-48).
    w_tp(r_mhartid());

    // switch to supervisor mode and jump to main(), passing the hartid in
    // a0 (start.c:50-51). tp holds the hartid written just above.
    // SAFETY: terminal `mret`; execution never returns past this point.
    unsafe { asm!("mv a0, tp", "mret", options(noreturn)) }
}
