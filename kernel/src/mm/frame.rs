//! Owned physical page frames (`kernel/kalloc.c`).

use super::addr::PhysAddr;
use super::kalloc;
use super::layout::PHYSTOP;
use crate::arch::PAGE_SIZE;

/// An owned, non-copyable 4 KiB physical page — the `void *` that
/// `kalloc` returns (kalloc.c:71-85). Dropping returns the page to the
/// allocator's freelist (`kfree`, kalloc.c:47-66); the type system makes
/// double frees unrepresentable.
pub struct PhysFrame {
    pa: PhysAddr,
}

impl PhysFrame {
    /// Adopt exclusive ownership of the page at `pa`.
    ///
    /// # Safety
    ///
    /// The caller must own the only claim to this page. In particular,
    /// `pa` must not already be represented by another `PhysFrame`, a live
    /// page-table mapping that retains ownership, or the allocator freelist.
    pub(crate) unsafe fn from_raw(pa: PhysAddr) -> Self {
        assert!(pa.0.is_multiple_of(PAGE_SIZE as u64), "kfree: unaligned");
        assert!(pa.0 >= kalloc::kernel_end().0, "kfree: below end");
        assert!(pa.0 < PHYSTOP.0, "kfree: above PHYSTOP");
        PhysFrame { pa }
    }

    /// The frame's physical address.
    pub fn addr(&self) -> PhysAddr {
        self.pa
    }

    /// Surrender ownership of the page to a page-table mapping: the frame
    /// will not be returned to the allocator on drop. The page-table code
    /// that takes the returned address becomes its owner, mirroring how
    /// C's `kalloc` result is owned by whatever maps it.
    pub fn leak(self) -> PhysAddr {
        let pa = self.pa;
        core::mem::forget(self);
        pa
    }
}

impl Drop for PhysFrame {
    fn drop(&mut self) {
        kalloc::free(self.pa);
    }
}
