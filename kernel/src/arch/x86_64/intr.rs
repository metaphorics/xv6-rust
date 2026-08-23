//! Local APIC timer and I/O APIC routing for q35.

use core::ptr;

use super::{outb, pci};

pub const TIMER_VECTOR: u8 = 32;
pub const UART0_IRQ: u32 = 4;
pub const VIRTIO0_IRQ: u32 = 11;

const LAPIC: usize = 0xfee0_0000;
const IOAPIC: usize = 0xfec0_0000;

const LAPIC_EOI: usize = 0x0b0;
const LAPIC_SVR: usize = 0x0f0;
const LAPIC_LVT_TIMER: usize = 0x320;
const LAPIC_TIMER_INITIAL: usize = 0x380;
const LAPIC_TIMER_DIVIDE: usize = 0x3e0;

fn lapic_write(offset: usize, value: u32) {
    // SAFETY: the kernel map covers the architectural LAPIC page exclusively.
    unsafe { ptr::write_volatile((LAPIC + offset) as *mut u32, value) };
}

fn ioapic_write(register: u32, value: u32) {
    // SAFETY: IOREGSEL/IOWIN are the serialized q35 I/O APIC register pair.
    unsafe {
        ptr::write_volatile(IOAPIC as *mut u32, register);
        ptr::write_volatile((IOAPIC + 0x10) as *mut u32, value);
    }
}

fn ioapic_read(register: u32) -> u32 {
    // SAFETY: as ioapic_write; reads are volatile device accesses.
    unsafe {
        ptr::write_volatile(IOAPIC as *mut u32, register);
        ptr::read_volatile((IOAPIC + 0x10) as *const u32)
    }
}

fn enable_lapic() {
    let mut apic_base_low: u32;
    let apic_base_high: u32;
    // SAFETY: IA32_APIC_BASE is the architectural APIC enable register.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") 0x1bu32,
            out("eax") apic_base_low,
            out("edx") apic_base_high,
            options(nostack)
        );
    }
    apic_base_low |= 1 << 11;
    // SAFETY: preserves the firmware-selected APIC base and enables xAPIC mode.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0x1bu32,
            in("eax") apic_base_low,
            in("edx") apic_base_high,
            options(nostack)
        );
    }
}

pub fn init() {
    enable_lapic();
    // The legacy PIC must not deliver duplicate interrupts alongside the APICs.
    outb(0x21, 0xff);
    outb(0xa1, 0xff);

    lapic_write(LAPIC_SVR, 0x100 | 47);
    lapic_write(LAPIC_TIMER_DIVIDE, 0x0b); // divide by 1
    lapic_write(LAPIC_LVT_TIMER, (1 << 17) | u32::from(TIMER_VECTOR));
    lapic_write(LAPIC_TIMER_INITIAL, 1_000_000); // 100 MHz APIC clock: about 10 ms

    let max_irq = (ioapic_read(1) >> 16) & 0xff;
    for irq in 0..=max_irq {
        ioapic_write(0x10 + irq * 2, 1 << 16);
        ioapic_write(0x11 + irq * 2, 0);
    }
    route(UART0_IRQ);
}

pub fn init_hart(_cpu: usize) {}

pub fn route(irq: u32) {
    ioapic_write(0x11 + irq * 2, 0);
    ioapic_write(0x10 + irq * 2, 32 + irq);
}

pub fn eoi() {
    lapic_write(LAPIC_EOI, 0);
}

pub fn virtio_irq() -> u32 {
    pci::interrupt_line().unwrap_or(VIRTIO0_IRQ)
}
