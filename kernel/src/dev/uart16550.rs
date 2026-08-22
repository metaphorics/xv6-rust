//! 16550A UART driver for the QEMU `virt` machine: byte-wide registers
//! memory-mapped at `UART0` (`memlayout.h:21`, `uart.c:14-22`).
//!
//! Two output paths, as in C: `uartwrite` for processes (serialized by
//! the transmit sleep lock, sleeping until the transmitter goes idle —
//! uart.c:78-96), and `putc_sync` for kernel printk and echo, which
//! polls and never sleeps (uart.c:99-120). `uartintr` wakes blocked
//! writers and hands input to the console (uart.c:138-155).

use core::ptr;

use crate::proc;
use crate::sync::{pop_off, push_off, SleepLock};

/// UART0 MMIO base (`memlayout.h:21`).
const UART0: usize = 0x1000_0000;

// Register offsets (`uart.c:25-39`). Some registers have different
// meanings for read vs write; see http://byterunner.com/16550.html.
const RHR: u8 = 0; // receive holding register (for input bytes)
const THR: u8 = 0; // transmit holding register (for output bytes)
const IER: u8 = 1; // interrupt enable register
const FCR: u8 = 2; // FIFO control register
const ISR: u8 = 2; // interrupt status register
const LCR: u8 = 3; // line control register
const LSR: u8 = 5; // line status register

const IER_RX_ENABLE: u8 = 0x01; // bit 0: receiver interrupts (uart.c:28)
const IER_TX_ENABLE: u8 = 0x02; // bit 1: transmitter interrupts (uart.c:29)
const FCR_FIFO_ENABLE: u8 = 0x01; // bit 0 (uart.c:31)
const FCR_FIFO_CLEAR: u8 = 0x06; // bits 1-2: clear both FIFOs (uart.c:32)
const LCR_EIGHT_BITS: u8 = 0x03; // bits 0-1: 8 data bits (uart.c:35)
const LCR_BAUD_LATCH: u8 = 0x80; // bit 7: baud-rate latch mode (uart.c:36)
const LSR_RX_READY: u8 = 0x01; // bit 0: input is waiting in RHR (uart.c:38)
const LSR_TX_IDLE: u8 = 0x20; // bit 5: THR can accept a byte (uart.c:39)

/// Serializes sending threads (`tx_lock`, uart.c:43).
static TX_LOCK: SleepLock = SleepLock::new();

/// The wait channel for threads blocked on the transmitter (`tx_chan`,
/// uart.c:44) — only its address matters.
static TX_CHAN: u8 = 0;

/// Read one UART register.
fn read_reg(reg: u8) -> u8 {
    // SAFETY: MMIO register read at the driver's fixed device address;
    // the 16550 has no other accessor, and reads have no side effects
    // beyond the device's own (the ISR read acknowledges interrupts).
    unsafe { ptr::read_volatile((UART0 + reg as usize) as *const u8) }
}

/// Write one UART register.
fn write_reg(reg: u8, value: u8) {
    // SAFETY: MMIO register write at the driver's fixed device address.
    unsafe { ptr::write_volatile((UART0 + reg as usize) as *mut u8, value) }
}

/// Program the UART: 38.4K baud, 8 data bits, FIFOs on, receive and
/// transmit interrupts enabled (`uartinit`, `uart.c:48-74`).
pub fn init() {
    // Disable interrupts.
    write_reg(IER, 0x00);

    // Special mode to set baud rate.
    write_reg(LCR, LCR_BAUD_LATCH);

    // LSB for 38.4K baud.
    write_reg(0, 0x03);

    // MSB for 38.4K baud.
    write_reg(1, 0x00);

    // Leave set-baud mode; 8 data bits, no parity.
    write_reg(LCR, LCR_EIGHT_BITS);

    // Reset and enable FIFOs.
    write_reg(FCR, FCR_FIFO_ENABLE | FCR_FIFO_CLEAR);

    // Enable transmit and receive interrupts.
    write_reg(IER, IER_TX_ENABLE | IER_RX_ENABLE);
}

/// Transmit `buf` to the uart, blocking while the transmitter is busy
/// (`uartwrite`, uart.c:78-96). Cannot be called from interrupts — the
/// writer sleeps — only from the write path.
pub fn write(buf: &[u8]) {
    TX_LOCK.acquire();

    for &b in buf {
        // Sleep until the transmit holding register is idle: check, and
        // if busy sleep on the channel `uartintr` wakes (uart.c:83-96).
        while read_reg(LSR) & LSR_TX_IDLE == 0 {
            proc::sleep0((&raw const TX_CHAN) as usize);
        }
        write_reg(THR, b);
    }

    TX_LOCK.release();
}

/// Write one byte without using interrupts, polling the line status
/// register until the transmit holding register is free (`uartputc_sync`,
/// uart.c:102-120). Brackets the poll with `push_off`/`pop_off` so an
/// interrupt on this hart cannot disturb the polling loop. This is the
/// printk and echo path (the panic freeze lives in `printk`).
pub fn putc_sync(c: u8) {
    push_off();
    // Wait for UART to set Transmit Holding Empty in LSR.
    while read_reg(LSR) & LSR_TX_IDLE == 0 {}
    write_reg(THR, c);
    pop_off();
}

/// Try to read one input character from the UART; `None` if none is
/// waiting (`uartgetc`, uart.c:124-133).
fn getc() -> Option<u8> {
    // Is input ready?
    if read_reg(LSR) & LSR_RX_READY != 0 {
        Some(read_reg(RHR))
    } else {
        None
    }
}

/// Handle a UART interrupt, raised because input has arrived, or the
/// UART is ready for more output, or both (`uartintr`, uart.c:138-155).
/// Called from `devintr`.
pub fn intr() {
    // Acknowledge the interrupt (uart.c:141).
    let _ = read_reg(ISR);

    if read_reg(LSR) & LSR_TX_IDLE != 0 {
        // UART finished transmitting; wake the sending thread
        // (uart.c:143-146).
        proc::wakeup((&raw const TX_CHAN) as usize);
    }

    // Read and process incoming characters, if any (uart.c:148-154).
    while let Some(c) = getc() {
        crate::dev::console::intr(c);
    }
}
