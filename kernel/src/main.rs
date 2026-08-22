#![no_std]
#![no_main]

//! xv6-rust kernel root.
//!
//! Control flow matches the C kernel (`entry.S:7`, `start.c:15`,
//! `main.c:11`): `entry` → `start` (machine mode) → `main` (supervisor
//! mode). Hart 0 brings up the console, the frame allocator and the
//! kernel page table, then the trap vector, the PLIC and interrupts;
//! the other harts wait for the release flag, repeat the per-hart steps
//! (`main.c:34-42`), and every hart parks in `wait_for_interrupt` until
//! the scheduler replaces the park (main.c:44).

#[macro_use]
mod printk;

mod arch;
mod cpu;
mod dev;
mod mm;
mod params;
mod sync;
mod trap;

use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch::riscv64::intr;

/// Set by hart 0 once its boot work is done; non-boot harts spin on it
/// (`__atomic_store_n(&started, 1, __ATOMIC_RELEASE)`, main.c:33).
static BOOT_RELEASE: AtomicBool = AtomicBool::new(false);

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
        // main.c leaves non-boot harts in scheduler(); until M4 they park
        // with interrupts on so their timer re-arms via kernelvec/clockintr
        // instead of spinning hot out of `wfi` with STIP pending forever.
        arch::intr_on();
        park();
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

    BOOT_RELEASE.store(true, Ordering::Release);

    // Interrupts on: in the C kernel this happens inside the scheduler
    // loop (proc.c); until the scheduler exists, the park below is what
    // runs with interrupts live.
    arch::intr_on();
    println!("xv6-rust: interrupts live");
    park();
}

/// Idle until the scheduler (M4) takes over: sleep until an interrupt is
/// pending, take it through the kernel trap vector, repeat.
fn park() -> ! {
    loop {
        arch::wait_for_interrupt();
    }
}
