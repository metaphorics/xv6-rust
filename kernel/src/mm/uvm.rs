//! User-space memory management (`vm.c`'s `uvm*` functions).
//!
//! Every operation works on a process's own page table, at user virtual
//! addresses below `p->sz`. The per-page loops, failure rollbacks, and
//! the page-boundary-chunked copies are ported exactly from C.

use crate::arch::{PageTable, Perm, TRAPFRAME, TRAMPOLINE};
use crate::arch::PAGE_SIZE;
use crate::err::Err;
use crate::mm::addr::{page_round_up, PhysAddr, VirtAddr};
use crate::mm::frame::PhysFrame;
use crate::mm::kalloc;

/// The page size, as the u64 the loops compute in.
const PAGE: u64 = PAGE_SIZE as u64;

/// Allocate PTEs and physical memory to grow a process from `oldsz` to
/// `newsz`, which need not be page-aligned (`uvmalloc`, vm.c:218-242).
/// `perm` joins `PTE_R | PTE_U` on every new mapping (the `xperm`
/// argument). Returns the new size; on failure rolls back to `oldsz`.
pub fn alloc(pt: &mut PageTable, oldsz: u64, newsz: u64, perm: Perm) -> Result<u64, Err> {
    if newsz < oldsz {
        return Ok(oldsz);
    }
    let oldsz = page_round_up(oldsz);
    let mut a = oldsz;
    while a < newsz {
        let frame = kalloc::alloc().ok_or_else(|| rollback(pt, a, oldsz))?;
        // The new page must read as zeroes (vm.c:229-231).
        zero_page(frame.addr());
        if pt
            .map_range(VirtAddr(a), frame.addr(), PAGE, Perm::R | Perm::U | perm)
            .is_err()
        {
            // The frame drops here, returning the page; then unwind the
            // mappings made so far (vm.c:233-239).
            return Err(rollback(pt, a, oldsz));
        }
        // Ownership moves into the mapping (the `mappages` handoff).
        frame.leak();
        a += PAGE;
    }
    Ok(newsz)
}

/// The `uvmdealloc` half of uvmalloc's error path: unmap the pages this
/// call added, then report `NoMem`.
fn rollback(pt: &mut PageTable, a: u64, oldsz: u64) -> Err {
    dealloc(pt, a, oldsz);
    Err::NoMem
}

/// Deallocate user pages to bring the process from `oldsz` down to
/// `newsz` (`uvmdealloc`, vm.c:249-260). Neither need be page-aligned,
/// and `newsz` may exceed `oldsz` (a no-op). Returns the new size.
pub fn dealloc(pt: &mut PageTable, oldsz: u64, newsz: u64) -> u64 {
    if newsz >= oldsz {
        return oldsz;
    }
    if page_round_up(newsz) < page_round_up(oldsz) {
        let npages = (page_round_up(oldsz) - page_round_up(newsz)) / PAGE;
        unmap_range(pt, page_round_up(newsz), npages as usize, true);
    }
    newsz
}

/// Copy a parent's whole user address space into a fresh child table
/// (`uvmcopy`, vm.c:299-326): for each mapped page, allocate a frame,
/// copy the bytes, map with the same permissions. Tolerates absent
/// page-table entries (vm.c:309-312's `continue` arms — this reference
/// allows lazily-allocated holes) and unwinds its own work on failure.
pub fn copy(old: &PageTable, new: &mut PageTable, sz: u64) -> Result<(), Err> {
    let mut i = 0u64;
    while i < sz {
        let Some(pte) = old.leaf_pte(i) else {
            i += PAGE;
            continue;
        };
        let pa = PhysAddr((pte >> 10) << 12);
        let perm = Perm::from_pte_flags(pte);
        let Some(frame) = kalloc::alloc() else {
            unmap_range(new, 0, (i / PAGE) as usize, true);
            return Err(Err::NoMem);
        };
        copy_page(pa, frame.addr());
        if new.map_range(VirtAddr(i), frame.addr(), PAGE, perm).is_err() {
            // The frame drops here (vm.c:321's kfree); unwind.
            unmap_range(new, 0, (i / PAGE) as usize, true);
            return Err(Err::NoMem);
        }
        frame.leak();
        i += PAGE;
    }
    Ok(())
}

/// Map `code` at virtual address 0 in a fresh one-page user image
/// (`uvminit`, upstream proc.c): one page, RWX|U, bytes copied in.
/// This reference execs /init instead; our M4 initcode uses the classic
/// shape (see the milestone report).
pub fn init(pt: &mut PageTable, code: &[u8]) {
    assert!(code.len() < PAGE as usize, "uvminit: more than a page");
    let Some(frame) = kalloc::alloc() else {
        panic!("uvminit: kalloc");
    };
    zero_page(frame.addr());
    let pa = frame.addr();
    // SAFETY: the frame is exclusively owned by this call between
    // `kalloc` and the mapping handoff below; nothing can alias it.
    unsafe {
        core::ptr::copy_nonoverlapping(code.as_ptr(), pa.0 as usize as *mut u8, code.len());
    }
    pt.map_range(VirtAddr(0), pa, PAGE, Perm::R | Perm::W | Perm::X | Perm::U)
        .expect("uvminit: mappages");
    frame.leak();
}

/// Free user memory pages, then the page-table pages (`uvmfree`,
/// vm.c:285-290). Consumes the table: its `Drop` runs the `freewalk`
/// half, which panics if any leaf mapping remains — call sites must
/// unmap leaves first.
pub fn free(mut pt: PageTable, sz: u64) {
    if sz > 0 {
        unmap_range(&mut pt, 0, (page_round_up(sz) / PAGE) as usize, true);
    }
    drop(pt);
}

/// Free a process's page table (`proc_freepagetable`, proc.c:203-215):
/// unmap the trampoline alias and the trapframe page without freeing
/// (the kernel owns the trampoline image; the trapframe frame is freed
/// by the owning `PhysFrame`), then `uvmfree` the user pages.
pub fn free_proc_table(mut pt: PageTable, sz: u64) {
    let _ = pt.take_leaf(TRAPFRAME.0);
    let _ = pt.take_leaf(TRAMPOLINE.0);
    free(pt, sz)
}

/// Remove `npages` of mappings starting at `va`, optionally freeing the
/// physical pages (`uvmunmap`, vm.c:194-213). Missing mappings are fine.
pub fn unmap_range(pt: &mut PageTable, va: u64, npages: usize, do_free: bool) {
    assert!(va % PAGE == 0, "uvmunmap: not aligned");
    for i in 0..npages {
        let a = va + i as u64 * PAGE;
        if let Some(pa) = pt.take_leaf(a) {
            if do_free {
                drop(PhysFrame::from_raw(pa));
            }
        }
    }
}

/// Look up a user virtual address, returning the physical address
/// (`walkaddr`, vm.c:122-139): requires a valid, user-accessible leaf.
pub fn walkaddr(pt: &PageTable, va: u64) -> Option<PhysAddr> {
    pt.walkaddr(va)
}


/// Copy from user to kernel (`copyin`, vm.c:383-405 minus this
/// reference's `vmfault` fallback, which belongs to the lazy-sbrk
/// machinery that joins with the usertests milestone): page-chunked,
/// `walkaddr`-validated. Unmapped user memory fails.
pub fn copy_in(pt: &PageTable, dst: &mut [u8], mut srcva: u64) -> Result<(), Err> {
    let mut dst = dst;
    while !dst.is_empty() {
        let va0 = srcva & !(PAGE - 1);
        let pa0 = walkaddr(pt, va0).ok_or(Err::BadArg)?;
        let n = ((PAGE - (srcva - va0)) as usize).min(dst.len());
        // SAFETY: `pa0` came from the user page table's leaf PTE, so the
        // page is mapped user-accessible and pinned by the table; the
        // kernel's identity map makes the physical range addressable
        // here. No alias exists because kernel buffers are never mapped
        // into user space.
        unsafe {
            core::ptr::copy_nonoverlapping((pa0.0 + srcva - va0) as usize as *const u8, dst.as_mut_ptr(), n);
        }
        dst = &mut dst[n..];
        srcva = va0 + PAGE;
    }
    Ok(())
}

/// Copy from kernel to user (`copyout`, vm.c:345-377): page-chunked,
/// `walkaddr`-validated, and — as in C — refusing to write over
/// read-only user pages (vm.c:366-368), which is how exec's text
/// protection and the copyout fstat-address tests behave.
pub fn copy_out(pt: &PageTable, mut dstva: u64, mut src: &[u8]) -> Result<(), Err> {
    while !src.is_empty() {
        let va0 = dstva & !(PAGE - 1);
        if va0 >= crate::arch::riscv64::vm::MAXVA {
            return Err(Err::BadArg);
        }
        let pa0 = walkaddr(pt, va0).ok_or(Err::BadArg)?;
        // forbid copyout over read-only user text pages.
        let pte = pt.leaf_pte(va0).ok_or(Err::BadArg)?;
        if !crate::arch::riscv64::vm::pte_writable(pte) {
            return Err(Err::BadArg);
        }
        let n = ((PAGE - (dstva - va0)) as usize).min(src.len());
        // SAFETY: as `copy_in` — a user page validated by `walkaddr`,
        // reached through the kernel identity map, with no kernel alias.
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr(),
                (pa0.0 + dstva - va0) as usize as *mut u8,
                n,
            );
        }
        src = &src[n..];
        dstva = va0 + PAGE;
    }
    Ok(())
}

/// Zero a freshly allocated page (`memset(mem, 0, PGSIZE)`).
fn zero_page(pa: PhysAddr) {
    // SAFETY: the caller holds the only claim to the page (fresh from
    // kalloc, before the mapping handoff), so the write cannot alias.
    unsafe { core::ptr::write_bytes(pa.0 as usize as *mut u8, 0, PAGE_SIZE) };
}

/// Copy one whole page between physical addresses (`memmove`, vm.c:319).
fn copy_page(src: PhysAddr, dst: PhysAddr) {
    // SAFETY: both pages are exclusively owned — `src` is mapped only in
    // the parent's table (process-private memory) and `dst` is fresh
    // from kalloc — and the kernel identity map covers both.
    unsafe {
        core::ptr::copy_nonoverlapping(src.0 as usize as *const u8, dst.0 as usize as *mut u8, PAGE_SIZE);
    }
}
