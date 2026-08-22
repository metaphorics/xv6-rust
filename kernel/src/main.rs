#![no_std]
#![no_main]

//! xv6-rust kernel root.
//!
//! Control flow matches the C kernel (`entry.S:7`, `start.c:15`,
//! `main.c:11`): `entry` → `start` (machine mode) → `main` (supervisor
//! mode). This milestone boots hart 0 to a UART banner and parks the
//! other harts; the tail of `main` grows into real subsystem bring-up.

#[macro_use]
mod printk;

mod arch;
mod cpu;
mod dev;
mod params;
mod sync;

use core::sync::atomic::{AtomicBool, Ordering};

/// Set by hart 0 once early boot is done; non-boot harts spin on it.
static BOOT_RELEASE: AtomicBool = AtomicBool::new(false);

/// Supervisor-mode entry, the `mret` target of `start`, with the hart id
/// in `a0`.
extern "C" fn main(hartid: usize) -> ! {
    if hartid != 0 {
        // Secondary harts wait for hart 0's early boot, then idle until a
        // later milestone gives them a scheduler.
        while !BOOT_RELEASE.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        loop {
            core::hint::spin_loop();
        }
    }

    dev::uart16550::init();
    println!("xv6-rust kernel is booting");
    BOOT_RELEASE.store(true, Ordering::Release);
    loop {
        core::hint::spin_loop();
    }
}
