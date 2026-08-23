//! x86_64 adapter for QEMU's q35 machine.

pub mod boot;
pub mod gdt;
pub mod intr;
pub mod pci;
pub mod swtch;
pub mod trapframe;
pub mod traps;
pub mod vm;

use core::arch::asm;

/// M9 is deliberately uniprocessor; the bootstrap CPU is CPU 0.
pub fn cpu_id() -> usize {
    0
}

pub fn intr_on() {
    // SAFETY: enables maskable interrupts after the caller has installed the IDT.
    unsafe { asm!("sti", options(nomem, nostack)) };
}

pub fn intr_off() {
    // SAFETY: disables maskable interrupts on this CPU.
    unsafe { asm!("cli", options(nomem, nostack)) };
}

pub fn intr_get() -> bool {
    let flags: u64;
    // SAFETY: reads RFLAGS through the current stack without changing machine state.
    unsafe { asm!("pushfq", "pop {}", out(reg) flags, options(preserves_flags)) };
    flags & (1 << 9) != 0
}

pub fn wait_for_interrupt() {
    // SAFETY: STI;HLT closes the scheduler's check/sleep race, and CLI
    // restores its interrupts-off invariant before returning.
    unsafe { asm!("sti", "hlt", "cli", options(nomem, nostack)) };
}

#[inline]
pub fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: the adapter calls this only for owned legacy I/O ports.
    unsafe { asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack)) };
    value
}

#[inline]
pub fn outb(port: u16, value: u8) {
    // SAFETY: the adapter calls this only for owned legacy I/O ports.
    unsafe { asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack)) };
}

#[inline]
pub fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: the PCI adapter serializes accesses to the configuration ports.
    unsafe { asm!("in eax, dx", in("dx") port, out("eax") value, options(nomem, nostack)) };
    value
}

#[inline]
pub fn outl(port: u16, value: u32) {
    // SAFETY: the PCI adapter serializes accesses to the configuration ports.
    unsafe { asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack)) };
}

pub fn uart_read(reg: u8) -> u8 {
    inb(0x3f8 + u16::from(reg))
}

pub fn uart_write(reg: u8, value: u8) {
    outb(0x3f8 + u16::from(reg), value);
}
