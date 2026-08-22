//! 16550A UART driver for the QEMU `virt` machine: byte-wide registers
//! memory-mapped at `UART0` (`memlayout.h:21`, `uart.c:14-22`).
//!
//! Transmit is interrupt-driven through a 32-byte ring: `put` enqueues
//! and `start` feeds the hardware; the transmit interrupt drains what
//! `start` left behind. This reference tree instead blocks writers on a
//! sleeplock and wakes them from `uartintr` (uart.c:42-43, 80-96,
//! 143-146) — a shape that needs `sleep`/`wakeup`, which arrive with
//! processes (M4). Until then the ring carries kernel output; see the
//! milestone report for the layering note.

use core::ptr;

use crate::arch;
use crate::dev::console;
use crate::sync::SpinLock;

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

/// Transmit ring size (`UART_TX_BUF_SIZE`).
const UART_TX_BUF_SIZE: u64 = 32;

/// The transmit ring: a byte buffer with monotonically growing read and
/// write cursors (`uart_tx_buf`/`uart_tx_r`/`uart_tx_w`).
struct Tx {
    buf: [u8; UART_TX_BUF_SIZE as usize],
    /// Next index to fill.
    w: u64,
    /// Next index to send.
    r: u64,
}

/// One lock serializes every ring access (`uart_tx_lock`).
static TX: SpinLock<Tx> = SpinLock::new(Tx {
    buf: [0; UART_TX_BUF_SIZE as usize],
    w: 0,
    r: 0,
});

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

/// If the transmit holding register is idle, feed it bytes from the ring
/// until it fills or the ring empties (`uartstart`). Transmit interrupts
/// are enabled while the ring still holds bytes the hardware has not
/// accepted, and disabled once it drains — the interrupt would otherwise
/// fire forever on an idle transmitter.
fn start(tx: &mut Tx) {
    loop {
        if tx.w == tx.r {
            // the ring is empty.
            let _ = read_reg(ISR);
            write_reg(IER, read_reg(IER) & !IER_TX_ENABLE);
            return;
        }

        if read_reg(LSR) & LSR_TX_IDLE == 0 {
            // the transmit holding register is full; it will interrupt
            // when it is ready for a new one.
            write_reg(IER, read_reg(IER) | IER_TX_ENABLE);
            return;
        }

        let c = tx.buf[(tx.r % UART_TX_BUF_SIZE) as usize];
        tx.r += 1;
        write_reg(THR, c);
    }
}

/// Write one output character through the transmit ring, waiting while
/// the ring is full (`uartputc`).
///
/// The wait only makes progress if the transmit interrupt can run, which
/// requires interrupts enabled; with interrupts disabled (trap handlers,
/// panic paths) the character goes out on the polled path instead, so
/// kernel output can never wedge on a full ring.
pub fn put(c: u8) {
    loop {
        {
            let mut tx = TX.lock();
            if tx.w - tx.r < UART_TX_BUF_SIZE {
                let slot = (tx.w % UART_TX_BUF_SIZE) as usize;
                tx.buf[slot] = c;
                tx.w += 1;
                start(&mut tx);
                return;
            }
        }

        if !arch::intr_get() {
            // no interrupt can drain the ring from here.
            putc_sync(c);
            return;
        }
        core::hint::spin_loop();
    }
}

/// Write one byte without using interrupts, polling the line status
/// register until the transmit holding register is free (`uartputc_sync`,
/// uart.c:102-120). Brackets the poll with `push_off`/`pop_off` so an
/// interrupt on this hart cannot disturb the polling loop. This is the
/// echo and panic path (the panic freeze lives in `printk`).
pub fn putc_sync(c: u8) {
    crate::sync::push_off();
    // wait for UART to set Transmit Holding Empty in LSR.
    while read_reg(LSR) & LSR_TX_IDLE == 0 {}
    write_reg(THR, c);
    crate::sync::pop_off();
}

/// Try to read one input character from the UART; `None` if none is
/// waiting (`uartgetc`, uart.c:124-133).
fn getc() -> Option<u8> {
    // is input ready?
    if read_reg(LSR) & LSR_RX_READY != 0 {
        Some(read_reg(RHR))
    } else {
        None
    }
}

/// Handle a UART interrupt, raised because input has arrived, or the
/// UART is ready for more output, or both (`uartintr`, uart.c:139-155).
/// Called from `devintr`.
pub fn intr() {
    // acknowledge the interrupt (uart.c:141).
    let _ = read_reg(ISR);

    // read and process incoming characters, if any (uart.c:148-154).
    while let Some(c) = getc() {
        console::intr(c);
    }

    // send buffered characters (the counterpart of waking the blocked
    // writer, uart.c:143-146).
    let mut tx = TX.lock();
    start(&mut tx);
}
