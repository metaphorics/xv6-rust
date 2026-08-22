#![no_std]
#![no_main]

//! xv6-rust kernel root.
//!
//! Control flow matches the C kernel (`entry.S:7`, `start.c:15`,
//! `main.c:11`): `entry` → `start` (machine mode) → `main` (supervisor
//! mode). Hart 0 brings up the console, the frame allocator and the
//! kernel page table, then turns paging on for every hart; the tail of
//! `main` grows into real subsystem bring-up (main.c:13-41).

#[macro_use]
mod printk;

mod arch;
mod cpu;
mod dev;
mod mm;
mod params;
mod sync;

use core::sync::atomic::{AtomicBool, Ordering};

/// Set by hart 0 once paging is on; non-boot harts spin on it
/// (`__atomic_store_n(&started, 1, __ATOMIC_RELEASE)`, main.c:30).
static BOOT_RELEASE: AtomicBool = AtomicBool::new(false);

/// Supervisor-mode entry, the `mret` target of `start`, with the hart id
/// in `a0`.
extern "C" fn main(hartid: usize) -> ! {
    if hartid != 0 {
        // Wait for hart 0's boot (main.c:32-33), then enable paging on
        // this hart too (main.c:36). A scheduler arrives with the proc
        // milestone; until then park with interrupts off.
        while !BOOT_RELEASE.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        mm::kernel_map::activate_hart();
        println!("hart {} running", hartid);
        loop {
            core::hint::spin_loop();
        }
    }

    dev::uart16550::init();
    println!("xv6-rust kernel is booting");
    mm::kalloc::init();
    mm::kernel_map::init();
    mm::kernel_map::activate_hart();
    println!("paging on");
    mm::selftest();
    BOOT_RELEASE.store(true, Ordering::Release);
    loop {
        core::hint::spin_loop();
    }
}
