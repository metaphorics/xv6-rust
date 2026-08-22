//! Kernel console output: `print!`/`println!` and the panic handler
//! (`kernel/printk.c`).
//!
//! Ordinary output is serialized by `sync::SpinLock` — the C `pr` lock —
//! and flows through the UART's interrupt-driven transmit ring. The
//! panic path deliberately bypasses both the lock and the ring and
//! drives the polled UART directly (`putc_sync`), so a panic while
//! another hart holds the lock still gets its message out.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::uart16550;
use crate::sync::SpinLock;

/// Set by the panic handler once the panic message is out. Any other hart
/// reaching a print spins instead of touching the UART, the freeze that
/// `panicked` provides in printk.c:130.
static PANICKED: AtomicBool = AtomicBool::new(false);

/// `fmt` sink for ordinary kernel output: through `consputc`, the
/// polled uart path printk uses (printk.c:41-42 -> console.c:35-45).
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
            crate::dev::console::putc(u32::from(b));
        }
        Ok(())
    }
}

/// The console lock (the `pr` spinlock, printk.c:22-24); a const
/// construction replaces `printkinit` (printk.c:148-152).
pub(crate) static PRINTK: SpinLock<Writer> = SpinLock::new(Writer);

/// `fmt` sink for the panic path: the polled UART only, so a wedged
/// transmit ring or a held lock cannot eat the panic message
/// (`panic`'s direct `uartputc_sync`, printk.c:137-146).
struct PanicWriter;

impl Write for PanicWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
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
        let mut writer = $crate::printk::PRINTK.lock();
        let _ = core::fmt::write(&mut *writer, format_args!($($arg)*));
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
/// spin (`panic`, printk.c:137-146). Lock-free and ring-free: only the
/// polled UART path, so it is safe regardless of what locks the panicking
/// hart held.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    let mut writer = PanicWriter;
    let _ = core::writeln!(writer, "PANIC: {info}");
    PANICKED.store(true, Ordering::Release);
    loop {
        core::hint::spin_loop();
    }
}
