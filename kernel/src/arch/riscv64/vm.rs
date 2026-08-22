//! Sv39 page tables (`kernel/vm.c`, the Sv39 definitions of `riscv.h`).
//!
//! Three-level radix tree, 512 PTEs per page-table page; a virtual
//! address is 27 bits of index (9 per level) over a 12-bit byte offset,
//! and `MAXVA` bounds the translatable half (riscv.h:389-417, vm.c:99-105).

use core::arch::asm;

use crate::arch::PAGE_SIZE;
use crate::mm::addr::{px, PhysAddr, VirtAddr};
use crate::mm::frame::PhysFrame;
use crate::mm::kalloc;

// PTE permission bits (riscv.h:395-399).
const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 8;

/// SATP mode for Sv39 (riscv.h:246).
const SATP_SV39: u64 = 8 << 60;

/// Highest usable virtual address, `1 << 38` (riscv.h:417).
pub const MAXVA: u64 = 1 << 38;

/// The trampoline's virtual address, `MAXVA - PGSIZE` (memlayout.h:48),
/// mapped in both the kernel and every user page table.
pub const TRAMPOLINE: VirtAddr = VirtAddr(MAXVA - PAGE_SIZE as u64);

/// The per-process trapframe page, just below the trampoline
/// (`TRAPFRAME = TRAMPOLINE - PGSIZE`, memlayout.h:60).
pub const TRAPFRAME: VirtAddr = VirtAddr(TRAMPOLINE.0 - PAGE_SIZE as u64);

/// Virtual address of process `p`'s kernel stack; each is followed by an
/// unmapped guard page (`KSTACK(p)`, memlayout.h:52).
pub const fn kstack(p: usize) -> VirtAddr {
    VirtAddr(TRAMPOLINE.0 - 2 * (p as u64 + 1) * PAGE_SIZE as u64)
}

/// Page permissions: the arch-neutral bit set of the seam, encoded here
/// as PTE_{R,W,X,U} (riscv.h:396-399).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Perm(u64);

impl Perm {
    /// Readable.
    pub const R: Perm = Perm(PTE_R);
    /// Writable.
    pub const W: Perm = Perm(PTE_W);
    /// Executable.
    pub const X: Perm = Perm(PTE_X);
    /// User-mode accessible (PTE_U, riscv.h:399).
    pub const U: Perm = Perm(PTE_U);

    /// The PTE bits of this permission set.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Build a `Perm` from raw PTE flag bits (`PTE_FLAGS(*pte)`,
    /// riscv.h:406 — the low 10 bits of a PTE), as `uvmcopy` does to
    /// carry a mapping's permissions into the copy.
    pub const fn from_pte_flags(flags: u64) -> Perm {
        Perm(flags & 0x3ff)
    }
}

impl core::ops::BitOr for Perm {
    type Output = Perm;

    fn bitor(self, rhs: Perm) -> Perm {
        Perm(self.0 | rhs.0)
    }
}

/// Does this raw PTE permit writes (`*pte & PTE_W`)? `copyout` uses the
/// answer to refuse writes over read-only user pages (vm.c:366-368).
pub fn pte_writable(pte: u64) -> bool {
    pte & PTE_W != 0
}

impl core::ops::BitOrAssign for Perm {
    fn bitor_assign(&mut self, rhs: Perm) {
        self.0 |= rhs.0;
    }
}

/// PA2PTE (riscv.h:402).
const fn pa2pte(pa: u64) -> u64 {
    (pa >> 12) << 10
}

/// PTE2PA (riscv.h:404).
const fn pte2pa(pte: u64) -> u64 {
    (pte >> 10) << 12
}

/// An Sv39 page table owning its pages (the `pagetable_t` of vm.c).
///
/// Dropping frees every page-table page via `freewalk` (vm.c:265-283),
/// which panics if any leaf mapping remains — exactly the C contract.
pub struct PageTable {
    root: PhysAddr,
}

impl PageTable {
    /// Allocate a zeroed root page (`kalloc` + `memset 0`, vm.c:24-27).
    /// `None` when out of memory.
    pub fn new() -> Option<Self> {
        let frame = kalloc::alloc()?;
        let root = frame.leak();
        zero(root);
        Some(PageTable { root })
    }

    /// Create PTEs for `[va, va+size)` referring to `[pa, pa+size)`
    /// (`mappages`, vm.c:147-175). `va`, `pa` and `size` must be
    /// page-aligned and non-zero and the range must not be mapped —
    /// violations panic, as in C. `Err(())` if an intermediate
    /// page-table page cannot be allocated.
    pub fn map_range(
        &mut self,
        va: VirtAddr,
        pa: PhysAddr,
        size: u64,
        perm: Perm,
    ) -> Result<(), ()> {
        let page = PAGE_SIZE as u64;
        if va.0 % page != 0 {
            panic!("mappages: va not aligned");
        }
        if size % page != 0 {
            panic!("mappages: size not aligned");
        }
        if size == 0 {
            panic!("mappages: size");
        }
        let mut a = va.0;
        let mut p = pa.0;
        let last = va.0 + size - page;
        loop {
            let pte = walk(self.root, a, true).ok_or(())?;
            // SAFETY: `walk` returned the leaf PTE slot inside a page
            // owned by this table (module invariant: page-table pages are
            // reached only through the owning PageTable), so the read and
            // write are unaliased.
            unsafe {
                if *pte & PTE_V != 0 {
                    panic!("mappages: remap");
                }
                *pte = pa2pte(p) | perm.bits() | PTE_V;
            }
            if a == last {
                break;
            }
            a += page;
            p += page;
        }
        Ok(())
    }

    /// Read the raw leaf PTE for `va` without allocating — `walk(pt, va,
    /// 0)` then inspect. `None` if any intermediate entry is missing or
    /// the leaf is invalid (vm.c:108-116).
    pub fn leaf_pte(&self, va: u64) -> Option<u64> {
        let pte = walk(self.root, va, false)?;
        // SAFETY: `walk` returned the leaf PTE slot inside a page owned
        // by this table (module invariant as in `map_range`), and the
        // read is unaliased.
        let pte = unsafe { *pte };
        (pte & PTE_V != 0).then_some(pte)
    }

    /// Remove the leaf mapping for `va`, returning the physical address
    /// it referred to (`*pte = 0` in uvmunmap, vm.c:211-212). `None` if
    /// there was no mapping — uvmunmap's "It's OK if the mappings don't
    /// exist". The returned address carries no ownership: the caller
    /// decides whether to free the frame.
    pub fn take_leaf(&mut self, va: u64) -> Option<PhysAddr> {
        let slot = walk(self.root, va, false)?;
        // SAFETY: `walk` returned the leaf PTE slot inside a page owned
        // by this table (module invariant as in `map_range`); the reads
        // and the clear are unaliased.
        let pte = unsafe { *slot };
        if pte & PTE_V == 0 {
            return None;
        }
        unsafe { *slot = 0 };
        Some(PhysAddr(pte2pa(pte)))
    }

    /// The `satp` value that activates this table
    /// (`MAKE_SATP(p->pagetable)`, riscv.h:248).
    pub fn satp_value(&self) -> u64 {
        SATP_SV39 | (self.root.0 >> 12)
    }

    /// Look up a user virtual address and return the physical address
    /// it maps to (`walkaddr`, vm.c:122-139): requires a present leaf
    /// with PTE_U. `None` if unmapped or not user-accessible. Can only
    /// be used to look up user pages.
    pub fn walkaddr(&self, va: u64) -> Option<PhysAddr> {
        let pte = self.leaf_pte(va)?;
        (pte & PTE_U != 0).then_some(PhysAddr(pte2pa(pte)))
    }

    /// Surrender the table to forever-static ownership (the kernel page
    /// table is never freed in C either, vm.c:16); the returned root
    /// address remains valid for `activate` on any hart.
    pub fn leak_root(self) -> PhysAddr {
        let root = self.root;
        core::mem::forget(self);
        root
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        freewalk(self.root);
    }
}

/// Turn on Sv39 paging with `root` on this hart: fence, install `satp`,
/// fence (`kvminithart`, vm.c:80-86; MAKE_SATP, riscv.h:248).
pub fn activate(root: PhysAddr) {
    let satp = SATP_SV39 | (root.0 >> 12);
    // SAFETY: `sfence.vma zero, zero` flushes the TLB (riscv.h:362-367)
    // and `csrw satp` installs the root; neither touches Rust memory, and
    // every hart's code, stack and statics are identity-mapped across the
    unsafe {
        asm!("sfence.vma zero, zero");
        asm!("csrw satp, {satp}", satp = in(reg) satp, options(nostack));
        asm!("sfence.vma zero, zero");
    }
}

/// Return the leaf PTE slot for `va` (`walk`, vm.c:99-120): descend
/// levels 2 then 1, following valid entries and otherwise allocating and
/// zeroing an intermediate page when `alloc`. `None` when an intermediate
/// entry is missing and `alloc` is false, or allocation fails.
fn walk(root: PhysAddr, va: u64, alloc: bool) -> Option<*mut u64> {
    assert!(va < MAXVA, "walk: va >= MAXVA");
    let mut table = table_of(root);
    for level in (1..=2).rev() {
        let i = px(level, va);
        let pte = pte_read(table, i);
        if pte & PTE_V != 0 {
            table = table_of(PhysAddr(pte2pa(pte)));
        } else {
            if !alloc {
                return None;
            }
            let frame = kalloc::alloc()?;
            let child = frame.leak();
            zero(child);
            pte_write(table, i, pa2pte(child.0) | PTE_V);
            table = table_of(child);
        }
    }
    // SAFETY: taking the address of the leaf PTE slot; no access occurs.
    Some(unsafe { core::ptr::addr_of_mut!((*table)[px(0, va)]) })
}

/// Recursively free page-table pages; all leaf mappings must already be
/// gone (`freewalk`, vm.c:265-283).
fn freewalk(root: PhysAddr) {
    let table = table_of(root);
    for i in 0..512 {
        let pte = pte_read(table, i);
        if pte & PTE_V != 0 && pte & (PTE_R | PTE_W | PTE_X) == 0 {
            // this PTE points to a lower-level page table.
            freewalk(PhysAddr(pte2pa(pte)));
            pte_write(table, i, 0);
        } else if pte & PTE_V != 0 {
            panic!("freewalk: leaf");
        }
    }
    drop(PhysFrame::from_raw(root));
}

/// The 512-PTE page-table page at `pa` (vm.c:268-269). A raw pointer, not
/// a reference: the same page is reached on every walk, and the accesses
/// below (`pte_read`/`pte_write`) are the only ones, so no Rust reference
/// ever outlives an access.
fn table_of(pa: PhysAddr) -> *mut [u64; 512] {
    pa.0 as usize as *mut [u64; 512]
}

/// Read PTE `i` of `table`.
///
/// SAFETY: every caller reaches `table` only through a `PageTable`'s
/// tree, whose pages were handed over by `PhysFrame::leak` (exclusive
/// ownership moved into the tree) and are touched nowhere else; `i < 512`.
fn pte_read(table: *mut [u64; 512], i: usize) -> u64 {
    unsafe { (*table)[i] }
}

/// Write PTE `i` of `table`. Same invariant as `pte_read`.
///
/// SAFETY: as `pte_read` — exclusive page ownership by the tree.
fn pte_write(table: *mut [u64; 512], i: usize, pte: u64) {
    unsafe { (*table)[i] = pte }
}

/// Zero the page at `pa` (`memset(pagetable, 0, PGSIZE)`, vm.c:27/111).
fn zero(pa: PhysAddr) {
    let table = table_of(pa);
    for i in 0..512 {
        pte_write(table, i, 0);
    }
}
