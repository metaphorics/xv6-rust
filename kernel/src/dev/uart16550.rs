//! 16550A UART driver for the QEMU `virt` machine: byte-wide registers
//! memory-mapped at `UART0` (`memlayout.h:21`, `uart.c:14-22`).
//!
//! This milestone owns the polled paths (`init`, `putc_sync`); the
//! interrupt-driven transmit ring and the receive path join with the
//! trap layer (`uartwrite`, `uartintr`, `uartgetc`).

use core::ptr;

/// UART0 MMIO base (`memlayout.h:21`).
const UART0: usize = 0x1000_0000;

// Register offsets (`uart.c:25-39`). Some registers have different
// meanings for read vs write; see http://byterunner.com/16550.html.
const THR: u8 = 0; // transmit holding register (for output bytes)
const IER: u8 = 1; // interrupt enable register
const FCR: u8 = 2; // FIFO control register
const LCR: u8 = 3; // line control register
const LSR: u8 = 5; // line status register

const IER_RX_ENABLE: u8 = 0x01; // bit 0: receiver interrupts (uart.c:28)
const IER_TX_ENABLE: u8 = 0x02; // bit 1: transmitter interrupts (uart.c:29)
const FCR_FIFO_ENABLE: u8 = 0x01; // bit 0 (uart.c:31)
const FCR_FIFO_CLEAR: u8 = 0x06; // bits 1-2: clear both FIFOs (uart.c:32)
const LCR_EIGHT_BITS: u8 = 0x03; // bits 0-1: 8 data bits (uart.c:35)
const LCR_BAUD_LATCH: u8 = 0x80; // bit 7: baud-rate latch mode (uart.c:36)
const LSR_TX_IDLE: u8 = 0x20; // bit 5: THR can accept a byte (uart.c:39)

/// Read one UART register.
fn read_reg(reg: u8) -> u8 {
    // SAFETY: a single byte-wide volatile load from the 16550's fixed
    // MMIO window; the device owns that memory and the volatile access
    // is the whole point of the read (uart.c:19).
    unsafe { ptr::read_volatile((UART0 + usize::from(reg)) as *const u8) }
}

/// Write one UART register.
fn write_reg(reg: u8, value: u8) {
    // SAFETY: a single byte-wide volatile store into the 16550's fixed
    // MMIO window; no Rust-owned memory is touched (uart.c:20).
    unsafe { ptr::write_volatile((UART0 + usize::from(reg)) as *mut u8, value) }
}

/// Program the UART: 38.4K baud, 8 data bits, FIFOs on, receive and
/// transmit interrupts enabled (`uartinit`, `uart.c:48-74`).
pub fn init() {
    // disable interrupts.
    write_reg(IER, 0x00);

    // special mode to set baud rate.
    write_reg(LCR, LCR_BAUD_LATCH);

    // LSB for baud rate of 38.4K.
    write_reg(0, 0x03);

    // MSB for baud rate of 38.4K.
    write_reg(1, 0x00);

    // leave set-baud mode, and set word length to 8 bits, no parity.
    write_reg(LCR, LCR_EIGHT_BITS);

    // reset and enable FIFOs.
    write_reg(FCR, FCR_FIFO_ENABLE | FCR_FIFO_CLEAR);

    // enable transmit and receive interrupts.
    write_reg(IER, IER_TX_ENABLE | IER_RX_ENABLE);
}

/// Write one byte without using interrupts, polling the line status
/// register until the transmit holding register is free. Safe to call
/// before the trap layer exists; this is the printk path
/// (`uartputc_sync`, `uart.c:102-120`).
///
/// The C version brackets the poll with push_off/pop_off and spins when
/// the kernel has panicked. The interrupt bracket needs `sync::SpinLock`
/// (M2), and the panic freeze lives one layer up, in `printk::Writer`.
pub fn putc_sync(c: u8) {
    // wait for UART to set Transmit Holding Empty in LSR.
    while read_reg(LSR) & LSR_TX_IDLE == 0 {}
    write_reg(THR, c);
}
