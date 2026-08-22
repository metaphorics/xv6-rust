//! Machine-mode entry point (`kernel/entry.S`).

use core::cell::UnsafeCell;

use crate::params::NCPU;

/// Per-hart boot stacks: 4096 bytes per hart, the whole array 16-byte
/// aligned, matching `__attribute__((aligned(16))) char
/// stack0[4096 * NCPU]` (`start.c:11`). `#[repr(align)]` cannot be
/// applied to a static directly, hence the newtype.
#[repr(C, align(16))]
struct BootStacks(UnsafeCell<[[u8; 4096]; NCPU]>);

// SAFETY: `BOOT_STACK` is written only by the entry assembly below (each
// hart uses its own disjoint 4096-byte row as its boot stack) and is never
// read or written from Rust, so no Rust code can observe it through a
// shared reference. Sharing it across harts is therefore sound.
unsafe impl Sync for BootStacks {}

/// The boot stacks. Memory below is scratch for the entry assembly.
static BOOT_STACK: BootStacks = BootStacks(UnsafeCell::new([[0; 4096]; NCPU]));

// qemu -kernel loads the kernel at 0x80000000 and makes every hart start
// executing there; kernel.ld places this code first, at that address
// (entry.S:1-7). Each hart computes the top of its own stack slot,
// sp = &BOOT_STACK + (hartid + 1) * 4096 (entry.S:11-17), and calls
// `start` in machine mode. If `start` ever returns, the hart spins.
// The multiply is a `slli` here — (hartid + 1) << 12 — because the
// release pipeline's integrated assembler rejects `mul` without an
// explicit Zmmul feature; the stride is a constant 4096 either way.
core::arch::global_asm!(
    ".section .text.entry, \"ax\", @progbits",
    ".globl _entry",
    "_entry:",
    "    la      sp, {stack}",
    "    csrr    a1, mhartid",
    "    addi    a1, a1, 1",
    "    slli    a1, a1, 12",
    "    add     sp, sp, a1",
    "    call    {start}",
    "1:  j       1b",
    stack = sym BOOT_STACK,
    start = sym crate::arch::riscv64::start::start,
);
