//! Physical page-frame allocator (`kernel/kalloc.c`): a singly linked
//! freelist threaded through the free pages themselves, protected by a
//! spin lock.

use super::addr::{PhysAddr, page_round_up};
use super::frame::PhysFrame;
use crate::arch::{PAGE_SIZE, PHYSTOP};
use crate::sync::SpinLock;

unsafe extern "C" {
    /// First address after the kernel image (`kernel.ld`
    /// `PROVIDE(end = .)`; kalloc.c:13).
    static end: u8;
}

/// The allocator state (`struct { spinlock; run *freelist; }`, kalloc.c:21-24).
/// The freelist head is kept as a plain physical address (0 = empty): the
/// link itself lives in the first word of each free page, C's
/// `struct run { struct run *next; }` (kalloc.c:17-19). An integer head
/// keeps the whole lock payload `Send` without any unsafe impl.
struct FreeList {
    head: usize,
}

static KMEM: SpinLock<FreeList> = SpinLock::new(FreeList { head: 0 });

/// Physical address of `end`, where free memory starts.
pub fn kernel_end() -> PhysAddr {
    // SAFETY: `end` is a linker-provided symbol; only its address is
    // meaningful, and taking an address is not a memory access.
    PhysAddr((&raw const end) as u64)
}

/// Build the freelist over all free RAM (`kinit` + `freerange`,
/// kalloc.c:26-41): every whole page from `PGROUNDUP(end)` to PHYSTOP.
pub fn init() {
    let mut pa = page_round_up(kernel_end().0);
    while pa + PAGE_SIZE as u64 <= PHYSTOP.0 {
        // SAFETY: initialization visits each whole RAM page exactly once
        // before the freelist is exposed, so no competing owner exists.
        drop(unsafe { PhysFrame::from_raw(PhysAddr(pa)) });
        pa += PAGE_SIZE as u64;
    }
}

/// Allocate one 4 KiB frame (`kalloc`, kalloc.c:71-85). The page is
/// junk-filled with `0x05` as in C — callers that need zeroed pages
/// (page tables, user memory) zero them explicitly. `None` when the
/// freelist is empty.
pub fn alloc() -> Option<PhysFrame> {
    let mut freelist = KMEM.lock();
    let head = freelist.head;
    if head == 0 {
        return None;
    }
    // SAFETY: `head` is a page currently owned by the freelist (invariant:
    // pages enter the freelist only through `free`, which surrenders all
    // other access), so its first word — the link to the next free page
    // (kalloc.c:75-79) — is valid to read.
    let next = unsafe { *(head as *const usize) };
    freelist.head = next;
    drop(freelist);

    let pa = PhysAddr(head as u64);
    fill(pa, 0x05); // fill with junk (kalloc.c:80)
    // SAFETY: removing `head` from the locked freelist transfers its sole
    // ownership to the returned frame.
    Some(unsafe { PhysFrame::from_raw(pa) })
}

/// Return `pa` to the freelist (`kfree`, kalloc.c:47-66). Called only by
/// `PhysFrame`'s drop, so the range checks of kalloc.c:52-54 hold by
/// construction.
pub(crate) fn free(pa: PhysAddr) {
    // Fill with junk to catch dangling references (kalloc.c:55). C pays
    // this on every free; this port pays it only in debug builds.
    if cfg!(debug_assertions) {
        fill(pa, 0x01);
    }

    let mut freelist = KMEM.lock();
    // SAFETY: the caller (PhysFrame::drop) has just given up the page's
    // ownership, so writing the link word cannot alias any live access
    // (kalloc.c:59-63).
    unsafe {
        *(pa.0 as usize as *mut usize) = freelist.head;
    }
    freelist.head = pa.0 as usize;
}

/// Fill the page at `pa` with `byte` (the `memset`s of kalloc.c:55,80).
fn fill(pa: PhysAddr, byte: u8) {
    // SAFETY: the page at `pa` is owned exclusively by the caller —
    // fresh off the freelist (alloc) or just surrendered to it (free) —
    // so no reference can alias this write.
    unsafe { core::ptr::write_bytes(pa.0 as usize as *mut u8, byte, PAGE_SIZE) };
}
