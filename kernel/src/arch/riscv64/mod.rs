//! riscv64 adapter for the QEMU `virt` machine.

pub mod entry;
pub mod intr;
pub mod kernelvec;
pub mod start;
pub mod swtch;
pub mod trampoline;
pub mod trapframe;
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

// ---- CSR access for the trap layer (`riscv.h` inline functions). Each
// helper is one instruction; none touch memory. ----

/// Read `sstatus` (`r_sstatus`, riscv.h:51-58).
pub fn r_sstatus() -> usize {
    let v;
    // SAFETY: reading a CSR into a local; no memory is touched.
    unsafe { asm!("csrr {v}, sstatus", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Write `sstatus` (`w_sstatus`, riscv.h:59-64).
pub fn w_sstatus(v: usize) {
    // SAFETY: writing a CSR; no memory is touched.
    unsafe { asm!("csrw sstatus, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Read `sepc`, the exception program counter (`r_sepc`, riscv.h:141-146).
pub fn r_sepc() -> usize {
    let v;
    // SAFETY: reading a CSR into a local; no memory is touched.
    unsafe { asm!("csrr {v}, sepc", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Write `sepc` (`w_sepc`, riscv.h:135-139).
pub fn w_sepc(v: usize) {
    // SAFETY: writing a CSR; no memory is touched.
    unsafe { asm!("csrw sepc, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Read `scause`, the trap cause (`r_scause`, riscv.h:268-274).
pub fn r_scause() -> usize {
    let v;
    // SAFETY: reading a CSR into a local; no memory is touched.
    unsafe { asm!("csrr {v}, scause", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Read `stval`, the trap value (fault address) (`r_stval`, riscv.h:277-283).
pub fn r_stval() -> usize {
    let v;
    // SAFETY: reading a CSR into a local; no memory is touched.
    unsafe { asm!("csrr {v}, stval", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Write `stvec`, the trap vector (`w_stvec`, riscv.h:181-186).
pub fn w_stvec(v: usize) {
    // SAFETY: writing a CSR; no memory is touched.
    unsafe { asm!("csrw stvec, {v}", v = in(reg) v, options(nomem, nostack)) };
}

/// Read `time`, the wall-clock cycle counter, readable in supervisor
/// mode once `mcounteren.TM` is set (`r_time`, riscv.h:300-306;
/// start.c:61-62).
pub fn r_time() -> usize {
    let v;
    // SAFETY: reading a CSR into a local; no memory is touched.
    unsafe { asm!("csrr {v}, time", v = out(reg) v, options(nomem, nostack)) };
    v
}

/// Read `satp`, the active page-table base (`r_satp`, riscv.h:53-58;
/// `prepare_return` saves it into the trapframe, trap.c:114).
pub fn r_satp() -> usize {
    let v;
    // SAFETY: reading a CSR; no memory is touched.
    unsafe {
        asm!("csrr {}, satp", out(reg) v, options(nomem, nostack));
    }
    v
}

/// sstc timer compare register, by number: the named `stimecmp` operand
/// is commented out in the reference in favor of `0x14d`
/// (riscv.h:196-211).
const STIMECMP: usize = 0x14d;

/// Write `stimecmp`, arming the next supervisor timer interrupt
/// (`w_stimecmp`, riscv.h:203-210).
pub fn w_stimecmp(v: usize) {
    // SAFETY: writing a CSR; no memory is touched.
    unsafe {
        asm!(
            "csrw {csr}, {v}",
            csr = const STIMECMP,
            v = in(reg) v,
            options(nomem, nostack)
        )
    };
}

/// Timer interval: 1_000_000 cycles is about a tenth of a second at the
/// `virt` machine's 10 MHz `time` frequency (`start.c:65`, `trap.c:179`).
pub const TIMER_INTERVAL: usize = 1_000_000;

/// Park the hart until an interrupt is pending (`wfi`). With `sstatus.SIE`
/// set the pending interrupt is taken through `stvec` on wake; the M3
/// boot park, replaced by the scheduler loop in M4.
pub fn wait_for_interrupt() {
    // SAFETY: `wfi` is a hint that sleeps the hart; no memory effect and
    // it cannot fault.
    unsafe { asm!("wfi", options(nomem, nostack)) };
}

pub fn uart_read(reg: u8) -> u8 {
    // SAFETY: UART0 is the adapter's byte-wide 16550 MMIO window.
    unsafe { core::ptr::read_volatile((0x1000_0000usize + reg as usize) as *const u8) }
}

pub fn uart_write(reg: u8, value: u8) {
    // SAFETY: UART0 is the adapter's byte-wide 16550 MMIO window.
    unsafe {
        core::ptr::write_volatile((0x1000_0000usize + reg as usize) as *mut u8, value);
    }
}
