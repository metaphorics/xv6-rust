//! The RISC-V Platform Level Interrupt Controller (`kernel/plic.c`).
//!
//! Registers are 32-bit MMIO in the PLIC window the kernel page table
//! identity-maps (`vm.c:36`).

use crate::mm::layout::PLIC;

/// PLIC MMIO base as a flat address (`PLIC`, memlayout.h:33), mapped
/// identity into the kernel address space (vm.c:36).
const BASE: usize = PLIC.0 as usize;

/// UART interrupt number (`UART0_IRQ`, memlayout.h:22).
pub const UART0_IRQ: u32 = 10;

/// VirtIO disk interrupt number (`VIRTIO0_IRQ`, memlayout.h:26).
pub const VIRTIO0_IRQ: u32 = 1;

/// This hart's S-mode enable register (`PLIC_SENABLE`, memlayout.h:36).
fn senable(hart: usize) -> usize {
    BASE + 0x2080 + hart * 0x100
}

/// This hart's S-mode priority threshold (`PLIC_SPRIORITY`,
/// memlayout.h:37).
fn spriority(hart: usize) -> usize {
    BASE + 0x201_000 + hart * 0x2000
}

/// This hart's S-mode claim/complete register (`PLIC_SCLAIM`,
/// memlayout.h:38).
fn sclaim(hart: usize) -> usize {
    BASE + 0x201_004 + hart * 0x2000
}

/// Read one 32-bit PLIC register.
fn read32(addr: usize) -> u32 {
    // SAFETY: a single volatile u32 load from the PLIC's fixed MMIO
    // window, mapped read-write in the kernel page table; the volatile
    // access is the whole point of the read (plic.c:15-16 pattern).
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Write one 32-bit PLIC register.
fn write32(addr: usize, value: u32) {
    // SAFETY: a single volatile u32 store into the PLIC's fixed MMIO
    // window; no Rust-owned memory is touched (plic.c:15-16 pattern).
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

/// Set the desired IRQ priorities non-zero — otherwise disabled
/// (`plicinit`, plic.c:11-17).
pub fn init() {
    write32(BASE + UART0_IRQ as usize * 4, 1);
    write32(BASE + VIRTIO0_IRQ as usize * 4, 1);
}

/// Enable the UART and virtio IRQs for this hart's S-mode context and
/// set its priority threshold to 0 (`plicinithart`, plic.c:19-30).
pub fn init_hart(hart: usize) {
    // set enable bits for this hart's S-mode (plic.c:24-26).
    write32(senable(hart), (1 << UART0_IRQ) | (1 << VIRTIO0_IRQ));

    // set this hart's S-mode priority threshold to 0 (plic.c:28-29).
    write32(spriority(hart), 0);
}

/// Ask the PLIC what interrupt we should serve; 0 means none
/// (`plic_claim`, plic.c:32-39).
pub fn claim(hart: usize) -> Option<u32> {
    let irq = read32(sclaim(hart));
    if irq == 0 { None } else { Some(irq) }
}

/// Tell the PLIC we've served this IRQ (`plic_complete`, plic.c:41-47).
pub fn complete(hart: usize, irq: u32) {
    write32(sclaim(hart), irq);
}
