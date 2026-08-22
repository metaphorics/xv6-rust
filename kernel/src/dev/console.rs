//! Console input and output, to the uart (`kernel/console.c`).
//!
//! Reads are line at a time (the read side joins with processes, M4);
//! this milestone owns the line discipline and the echo path. Special
//! input characters: newline ends a line, control-h backspaces,
//! control-u kills the line, control-d is end of file, control-p prints
//! the process list (console.c:1-9).

use crate::dev::uart16550;
use crate::sync::SpinLock;

/// Erase the last output character: a sentinel one bit above any byte
/// (`BACKSPACE`, console.c:25).
pub const BACKSPACE: u32 = 0x100;

/// Input buffer size (`INPUT_BUF_SIZE`, console.c:51).
const INPUT_BUF_SIZE: usize = 128;

/// Control-x (`C(x)`, console.c:26).
fn ctrl(x: u8) -> u32 {
    u32::from(x.wrapping_sub(b'@'))
}

/// The console: an input circular buffer under one lock (`cons`,
/// console.c:47-56).
struct Cons {
    /// Input buffer.
    buf: [u8; INPUT_BUF_SIZE],
    /// Read index (`r`, console.c:53).
    r: u32,
    /// Write index (`w`, console.c:54).
    w: u32,
    /// Edit index (`e`, console.c:55).
    e: u32,
}

/// A const-constructed lock replaces `consoleinit`'s `initlock`
/// (console.c:195).
static CONS: SpinLock<Cons> = SpinLock::new(Cons {
    buf: [0; INPUT_BUF_SIZE],
    r: 0,
    w: 0,
    e: 0,
});

/// Send one character to the uart on the polled path, without using
/// interrupts or sleeping — safe to be called from interrupt handlers,
/// e.g. to echo input characters (`consputc`, console.c:35-45).
pub fn putc(c: u32) {
    if c == BACKSPACE {
        // if the user typed backspace, overwrite with a space.
        uart16550::putc_sync(b'\x08');
        uart16550::putc_sync(b' ');
        uart16550::putc_sync(b'\x08');
    } else {
        uart16550::putc_sync(c as u8);
    }
}

/// The console input interrupt handler, called by `uartintr` for each
/// input character: erase/kill processing, echo, append to the input
/// buffer (`consoleintr`, console.c:147-190).
pub fn intr(c: u8) {
    let mut cons = CONS.lock();
    let c = u32::from(c);

    if c == ctrl(b'P') {
        // Print process list (console.c:152-154): `procdump` joins with
        // the process table (M4); there is nothing to dump yet.
    } else if c == ctrl(b'U') {
        // Kill line (console.c:155-161).
        while cons.e != cons.w && cons.buf[((cons.e - 1) as usize) % INPUT_BUF_SIZE] != b'\n' {
            cons.e -= 1;
            putc(BACKSPACE);
        }
    } else if c == ctrl(b'H') || c == 0x7f {
        // Backspace, and the delete key (console.c:162-168).
        if cons.e != cons.w {
            cons.e -= 1;
            putc(BACKSPACE);
        }
    } else if c != 0 && cons.e - cons.r < INPUT_BUF_SIZE as u32 {
        let c = if c == u32::from(b'\r') {
            u32::from(b'\n')
        } else {
            c
        };

        // echo back to the user (console.c:173-174).
        putc(c);

        // store for consumption by the read path (console.c:176-177).
        let slot = cons.e as usize % INPUT_BUF_SIZE;
        cons.buf[slot] = c as u8;
        cons.e += 1;

        if c == u32::from(b'\n') || c == ctrl(b'D') || cons.e - cons.r == INPUT_BUF_SIZE as u32 {
            // a whole line (or end-of-file, or a full buffer) has
            // arrived; the wakeup of the read path (console.c:179-184)
            // joins with sleep/wakeup (M4).
            cons.w = cons.e;
        }
    }
}

/// Bring up the console (`consoleinit`, console.c:193-203): the input
/// lock is the const static above, the uart comes up here, and the
/// `devsw[CONSOLE]` read/write wiring (console.c:199-202) joins with the
/// file table (M5).
pub fn init() {
    uart16550::init();
}
