//! Address newtypes and page arithmetic (`riscv.h:389-417`).

use crate::arch::PAGE_SIZE;

/// A physical address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

/// A virtual address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct VirtAddr(pub u64);

/// Round up to a page boundary (`PGROUNDUP`, riscv.h:392).
pub const fn page_round_up(a: u64) -> u64 {
    (a + PAGE_SIZE as u64 - 1) & !((PAGE_SIZE - 1) as u64)
}

/// The 9-bit page-table index of `va` at `level` (0 = leaf);
/// `PXSHIFT(level) = 12 + 9*level` (riscv.h:409-411).
pub const fn px(level: u32, va: u64) -> usize {
    ((va >> (12 + 9 * level)) & 0x1ff) as usize
}
