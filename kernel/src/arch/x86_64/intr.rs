//! Local APIC timer and I/O APIC routing for q35.

use core::ptr;

use super::{outb, pci};

pub const TIMER_VECTOR: u8 = 32;
pub const UART0_IRQ: u32 = 4;
pub const VIRTIO0_IRQ: u32 = 11;

const LAPIC: usize = 0xfee0_0000;
const IOAPIC: usize = 0xfec0_0000;

const LAPIC_ID: usize = 0x020;
const LAPIC_EOI: usize = 0x0b0;
const LAPIC_SVR: usize = 0x0f0;
const LAPIC_ICR_LOW: usize = 0x300;
const LAPIC_ICR_HIGH: usize = 0x310;
const LAPIC_LVT_TIMER: usize = 0x320;
const LAPIC_TIMER_INITIAL: usize = 0x380;
const LAPIC_TIMER_DIVIDE: usize = 0x3e0;

const ICR_DELIVERY_PENDING: u32 = 1 << 12;
const ICR_ASSERT: u32 = 1 << 14;
const ICR_LEVEL: u32 = 1 << 15;
const ICR_INIT: u32 = 0x500;
const ICR_STARTUP: u32 = 0x600;

fn lapic_write(offset: usize, value: u32) {
    // SAFETY: the kernel map covers the architectural LAPIC page exclusively.
    unsafe { ptr::write_volatile((LAPIC + offset) as *mut u32, value) };
}

fn lapic_read(offset: usize) -> u32 {
    // SAFETY: the kernel map covers the architectural LAPIC page exclusively.
    unsafe { ptr::read_volatile((LAPIC + offset) as *const u32) }
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

fn configure_lapic() {
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
    lapic_write(LAPIC_SVR, 0x100 | 47);
    lapic_write(LAPIC_TIMER_DIVIDE, 0x0b);
    lapic_write(LAPIC_LVT_TIMER, (1 << 17) | u32::from(TIMER_VECTOR));
    lapic_write(LAPIC_TIMER_INITIAL, 1_000_000);
}

pub fn init() {
    configure_lapic();
    // The legacy PIC must not deliver duplicate interrupts alongside the APICs.
    outb(0x21, 0xff);
    outb(0xa1, 0xff);

    let max_irq = (ioapic_read(1) >> 16) & 0xff;
    for irq in 0..=max_irq {
        ioapic_write(0x10 + irq * 2, 1 << 16);
        ioapic_write(0x11 + irq * 2, 0);
    }
    route(UART0_IRQ);
}

pub fn init_hart(_cpu: usize) {
    configure_lapic();
}

pub fn local_apic_id() -> u32 {
    lapic_read(LAPIC_ID) >> 24
}

fn wait_icr() {
    while lapic_read(LAPIC_ICR_LOW) & ICR_DELIVERY_PENDING != 0 {
        core::hint::spin_loop();
    }
}

fn apic_delay() {
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }
}

fn send_ipi(apic_id: u32, command: u32) {
    wait_icr();
    lapic_write(LAPIC_ICR_HIGH, apic_id << 24);
    lapic_write(LAPIC_ICR_LOW, command);
    wait_icr();
}

pub fn start_ap(apic_id: u32, vector: u8) {
    send_ipi(apic_id, ICR_INIT | ICR_LEVEL | ICR_ASSERT);
    apic_delay();
    send_ipi(apic_id, ICR_INIT | ICR_LEVEL);
    apic_delay();
    for _ in 0..2 {
        send_ipi(apic_id, ICR_STARTUP | u32::from(vector));
        apic_delay();
    }
}

pub fn route(irq: u32) {
    ioapic_write(0x11 + irq * 2, local_apic_id() << 24);
    ioapic_write(0x10 + irq * 2, 32 + irq);
}

pub fn eoi() {
    lapic_write(LAPIC_EOI, 0);
}

pub fn virtio_irq() -> u32 {
    pci::interrupt_line().unwrap_or(VIRTIO0_IRQ)
}
