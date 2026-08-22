//! Kernel console output: `print!`/`println!` and the panic handler
//! (`kernel/printk.c`).
//!
//! The writer is deliberately unsynchronized: at this milestone only one
//! hart prints and interrupts are off. The `printk.c` pr lock
//! (`sync::SpinLock`) wraps `Writer` once concurrent printers exist; the
//! macro interface is final.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::uart16550;

/// Set by the panic handler once the panic message is out. Any other hart
/// reaching a print spins instead of touching the UART, the freeze that
/// `panicked` provides in `printk.c:130`.
static PANICKED: AtomicBool = AtomicBool::new(false);

/// `fmt` sink that emits through the UART's polled path.
pub struct Writer;

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if PANICKED.load(Ordering::Acquire) {
            // The kernel is dead; only the panicking hart may still print.
            loop {
                core::hint::spin_loop();
            }
        }
        for &b in s.as_bytes() {
            uart16550::putc_sync(b);
        }
        Ok(())
    }
}

/// Print to the console without a trailing newline.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let mut writer = $crate::printk::Writer;
        let _ = core::fmt::write(&mut writer, format_args!($($arg)*));
    }};
}

/// Print to the console with a trailing newline.
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)+) => {
        $crate::print!("{}\n", format_args!($($arg)+))
    };
}

/// Print the panic message, freeze every other hart's console output, and
/// spin (`panic`, `printk.c:125-133`). Uses only the polled UART path.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    let mut writer = Writer;
    let _ = core::writeln!(writer, "PANIC: {info}");
    PANICKED.store(true, Ordering::Release);
    loop {
        core::hint::spin_loop();
    }
}
