#![no_std]
#![no_main]

//! xv6-rust kernel root.
//!
//! Control flow matches the C kernel (`entry.S:7`, `start.c:15`,
//! `main.c:11`): `entry` → `start` (machine mode) → `main` (supervisor
//! mode). Hart 0 brings up the console, the frame allocator and the
//! kernel page table, then the trap vector, the PLIC and interrupts;
//! the other harts wait for the release flag, repeat the per-hart steps
//! (`main.c:34-42`); every hart then enters the process scheduler, the
//! idle loop from here on (main.c:44).

#[macro_use]
mod printk;

mod arch;
mod cpu;
mod dev;
mod err;
mod mm;
mod params;
mod proc;
mod sync;
mod syscall;
mod sysproc;
mod trap;

use core::sync::atomic::{AtomicBool, Ordering};

/// Set by hart 0 once its boot work is done; non-boot harts spin on it
/// (`__atomic_store_n(&started, 1, __ATOMIC_RELEASE)`, main.c:33).
static BOOT_RELEASE: AtomicBool = AtomicBool::new(false);

use crate::arch::riscv64::intr;

/// Supervisor-mode entry, the `mret` target of `start`, with the hart id
/// in `a0`.
extern "C" fn main(hartid: usize) -> ! {
    if hartid != 0 {
        // Wait for hart 0's boot (main.c:34-35).
        while !BOOT_RELEASE.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }

        println!("hart {} running", hartid);
        mm::kernel_map::activate_hart(); // turn on paging (main.c:39)
        trap::init_hart(); // install kernel trap vector (main.c:40)
        intr::init_hart(hartid); // ask PLIC for device interrupts (main.c:41)
        proc::scheduler(); // run processes (main.c:44)
    }

    dev::console::init(); // consoleinit (main.c:14)
    println!();
    println!("xv6-rust kernel is booting"); // main.c:17
    println!();
    mm::kalloc::init(); // physical page allocator (main.c:19)
    mm::kernel_map::init(); // create kernel page table (main.c:20)
    mm::kernel_map::activate_hart(); // turn on paging (main.c:21)
    mm::selftest();
    trap::init_hart(); // install kernel trap vector (main.c:24)
    intr::init(); // set up interrupt controller (main.c:25)
    intr::init_hart(0); // ask PLIC for device interrupts (main.c:26)
    proc::user_init(); // first user process (main.c:30)

    BOOT_RELEASE.store(true, Ordering::Release);

    proc::scheduler(); // run processes (main.c:44)
}
