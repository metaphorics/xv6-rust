#![no_std]
#![no_main]

//! M0 kernel root: proves the riscv64gc build pipeline only.
//!
//! M1 replaces this with the real riscv64 entry (`global_asm!` boot entry,
//! `arch::start`, UART console, printk). Until then the crate checks for
//! the target but does not link, since no entry symbol exists yet.

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
