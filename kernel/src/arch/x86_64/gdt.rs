//! Per-CPU GDT and TSS for privilege-level stack switching.

use core::cell::UnsafeCell;

use crate::params::NCPU;

pub const KERNEL_CODE: u16 = 0x08;
pub const KERNEL_DATA: u16 = 0x10;
const TSS_SELECTOR: u16 = 0x28;

#[repr(C, packed)]
struct Tss {
    reserved0: u32,
    rsp: [u64; 3],
    reserved1: u64,
    ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    iomap_base: u16,
}

impl Tss {
    const ZERO: Self = Self {
        reserved0: 0,
        rsp: [0; 3],
        reserved1: 0,
        ist: [0; 7],
        reserved2: 0,
        reserved3: 0,
        iomap_base: core::mem::size_of::<Tss>() as u16,
    };
}

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

#[repr(align(4096))]
struct Shared<T>(UnsafeCell<T>);
// SAFETY: each page is accessed only by its owning CPU after BSP initialization.
unsafe impl<T> Sync for Shared<T> {}

static TSS: [Shared<Tss>; NCPU] = [const { Shared(UnsafeCell::new(Tss::ZERO)) }; NCPU];
static GDT: [Shared<[u64; 7]>; NCPU] = [const { Shared(UnsafeCell::new([0; 7])) }; NCPU];

const TABLE_VA_BASE: u64 = super::vm::KERNEL_HIGH_BASE + 3 * 1024 * 1024;

pub fn gdt_addr(cpu: usize) -> u64 {
    GDT[cpu].0.get() as u64
}

pub fn tss_addr(cpu: usize) -> u64 {
    TSS[cpu].0.get() as u64
}

pub fn gdt_va(cpu: usize) -> u64 {
    TABLE_VA_BASE - cpu as u64 * 2 * 4096
}

pub fn tss_va(cpu: usize) -> u64 {
    gdt_va(cpu) - 4096
}

pub fn init(cpu: usize, initial_rsp0: u64) {
    set_rsp0_for(cpu, initial_rsp0);
    let gdt = GDT[cpu].0.get();
    let tss_base = tss_va(cpu);
    let limit = (core::mem::size_of::<Tss>() - 1) as u64;
    let tss_low = (limit & 0xffff)
        | ((tss_base & 0x00ff_ffff) << 16)
        | (0x89 << 40)
        | (((limit >> 16) & 0xf) << 48)
        | (((tss_base >> 24) & 0xff) << 56);
    // SAFETY: this CPU exclusively initializes its static descriptor row.
    unsafe {
        *gdt = [
            0,
            0x00af_9a00_0000_ffff,
            0x00af_9200_0000_ffff,
            0x00af_f200_0000_ffff,
            0x00af_fa00_0000_ffff,
            tss_low,
            tss_base >> 32,
        ];
    }
    let gdtr = DescriptorTablePointer {
        limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
        base: gdt_va(cpu),
    };
    // SAFETY: both high aliases are mapped to this CPU's static pages.
    unsafe {
        core::arch::asm!(
            "lgdt [{}]",
            "mov ax, {data}",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "xor eax, eax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ax, {tss}",
            "ltr ax",
            in(reg) &gdtr,
            data = const KERNEL_DATA,
            tss = const TSS_SELECTOR,
            out("rax") _,
            options(nostack)
        );
    }
}

fn set_rsp0_for(cpu: usize, stack_top: u64) {
    // SAFETY: callers update only their CPU's TSS while interrupts are disabled.
    unsafe {
        core::ptr::addr_of_mut!((*TSS[cpu].0.get()).rsp[0]).write_unaligned(stack_top);
    }
}

pub fn set_rsp0(stack_top: u64) {
    set_rsp0_for(super::cpu_id(), stack_top);
}
