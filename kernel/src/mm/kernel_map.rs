//! The kernel address space: `kvmmake` (vm.c:20-51), `proc_mapstacks`
//! (proc.c:31-44) and `kvminithart` (vm.c:73-86).
//!
//! The kernel page table is shared by all harts and never freed (the C
//! `kernel_pagetable` global, vm.c:16); here it is built once by hart 0
//! and its root published as a raw physical address.

use core::sync::atomic::{AtomicU64, Ordering};

use super::addr::{PhysAddr, VirtAddr};
use super::kalloc;
use super::layout::{KERNBASE, PHYSTOP, PLIC, PLIC_SIZE, UART0, VIRTIO0};
use crate::arch::{self, kstack, PageTable, Perm};
use crate::params::NPROC;

unsafe extern "C" {
    /// End of kernel text, `kernel.ld` `PROVIDE(etext = .)` (vm.c:12).
    static etext: u8;
}

/// Physical root of the kernel page table, published once `init` has
/// built it (`kernel_pagetable`, vm.c:16). 0 means not yet built.
static KERNEL_ROOT: AtomicU64 = AtomicU64::new(0);

/// The page size, as the `u64` the mapping code computes in.
const PAGE: u64 = arch::PAGE_SIZE as u64;

/// Build the kernel page table (`kvminit`/`kvmmake`, vm.c:20-51).
pub fn init() {
    let mut pt = PageTable::new().expect("kvmmake: no page for root");

    // uart registers (vm.c:30).
    kvmmap(&mut pt, VirtAddr(UART0.0), UART0, PAGE, Perm::R | Perm::W);

    // virtio mmio disk interface (vm.c:33).
    kvmmap(&mut pt, VirtAddr(VIRTIO0.0), VIRTIO0, PAGE, Perm::R | Perm::W);

    // PLIC (vm.c:36).
    kvmmap(&mut pt, VirtAddr(PLIC.0), PLIC, PLIC_SIZE, Perm::R | Perm::W);

    // kernel text, executable and read-only (vm.c:39).
    // SAFETY: `etext` is a linker-provided symbol; only its address is
    // meaningful, and taking an address is not a memory access.
    let etext_pa = PhysAddr((&raw const etext) as u64);
    kvmmap(
        &mut pt,
        VirtAddr(KERNBASE.0),
        KERNBASE,
        etext_pa.0 - KERNBASE.0,
        Perm::R | Perm::X,
    );

    // kernel data and the physical RAM we'll make use of (vm.c:42-43).
    kvmmap(
        &mut pt,
        VirtAddr(etext_pa.0),
        etext_pa,
        PHYSTOP.0 - etext_pa.0,
        Perm::R | Perm::W,
    );

    // The trampoline mapping (TRAMPOLINE -> trampoline page, vm.c:46-47)
    // lands in M4, when the trampoline page itself exists.

    // allocate and map a kernel stack for each process, each under an
    // unmapped guard page (proc_mapstacks, proc.c:33-44).
    for p in 0..NPROC {
        let frame = kalloc::alloc().expect("proc_mapstacks: kalloc");
        kvmmap(
            &mut pt,
            kstack(p),
            frame.leak(),
            PAGE,
            Perm::R | Perm::W,
        );
    }

    KERNEL_ROOT.store(pt.leak_root().0, Ordering::Release);
}

/// Switch this hart onto the kernel page table (`kvminithart`,
/// vm.c:73-86).
pub fn activate_hart() {
    let root = KERNEL_ROOT.load(Ordering::Acquire);
    assert!(root != 0, "kvminithart before kvminit");
    arch::activate(PhysAddr(root));
}

/// Add a mapping to the kernel page table, panicking on failure —
/// `kvmmap` (vm.c:57-63): "only used when booting".
fn kvmmap(pt: &mut PageTable, va: VirtAddr, pa: PhysAddr, size: u64, perm: Perm) {
    pt.map_range(va, pa, size, perm).expect("kvmmap");
}
