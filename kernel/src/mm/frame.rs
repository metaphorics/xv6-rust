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
    /// Adopt the page at `pa`. Crate-private: callers must hold the only
    /// claim to the page — the allocator (freerange/alloc) or a page-table
    /// tree being unwound. The `kfree` range checks (kalloc.c:52-54) hold
    /// by construction and are asserted in debug builds.
    pub(crate) fn from_raw(pa: PhysAddr) -> Self {
        debug_assert!(pa.0 % PAGE_SIZE as u64 == 0, "kfree: unaligned");
        debug_assert!(pa.0 >= kalloc::kernel_end().0, "kfree: below end");
        debug_assert!(pa.0 < PHYSTOP.0, "kfree: above PHYSTOP");
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
