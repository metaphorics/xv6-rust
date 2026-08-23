//! Per-CPU GDT and TSS for privilege-level stack switching.

use core::cell::UnsafeCell;

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

struct Shared<T>(UnsafeCell<T>);
// SAFETY: M9 has one CPU; interrupt masking serializes updates to these tables.
unsafe impl<T> Sync for Shared<T> {}

static TSS: Shared<Tss> = Shared(UnsafeCell::new(Tss::ZERO));
static GDT: Shared<[u64; 7]> = Shared(UnsafeCell::new([0; 7]));

pub fn init(initial_rsp0: u64) {
    set_rsp0(initial_rsp0);
    let tss_base = TSS.0.get() as u64;
    let limit = (core::mem::size_of::<Tss>() - 1) as u64;
    let tss_low = (limit & 0xffff)
        | ((tss_base & 0x00ff_ffff) << 16)
        | (0x89 << 40)
        | (((limit >> 16) & 0xf) << 48)
        | (((tss_base >> 24) & 0xff) << 56);
    // SAFETY: the bootstrap CPU is the only writer during initialization.
    unsafe {
        *GDT.0.get() = [
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
        base: GDT.0.get() as u64,
    };
    // SAFETY: descriptors are fully initialized and remain static forever.
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

pub fn set_rsp0(stack_top: u64) {
    // SAFETY: M9 is UP and callers update rsp0 only with interrupts disabled.
    unsafe { core::ptr::addr_of_mut!((*TSS.0.get()).rsp[0]).write_unaligned(stack_top) };
}
