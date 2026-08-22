//! The first user program (`user/initcode.S`'s role in upstream xv6).
//!
//! This reference execs /init from `forkret` instead (proc.c:522-537),
//! which needs the file system; until exec lands (M6) the first process
//! runs this hand-assembled loop, and M6 replaces these bytes with the
//! exec variant through the same `uvm::init` mechanism.
//!
//! Program, as riscv64 machine code (assembled and cross-checked with
//! `riscv64-unknown-elf-as`; register/imm encodings in comments):
//!
//! ```text
//! start:                          ; va 0x00, one RWX|U page; sp = PGSIZE
//!   li    a7, 16                  ; SYS_write (syscall.h:16)
//!   li    a0, 1                   ; fd 1 (stdout)
//!   li    a1, 32                  ; &msg — 8 instructions * 4 = 0x20
//!   li    a2, 16                  ; sizeof "hello from user\n"
//!   ecall
//!   li    a7, 11                  ; SYS_getpid (syscall.h:11)
//!   ecall
//!   j     start                   ; -28: 0x1c back to 0x00
//! msg:   .string "hello from user\n"  ; va 0x20, 16 chars + nul
//! ```
//!
//! `li` is `addi rd, x0, imm` (imm[11:0]|rs1=0|f3=0|rd|op=0x13); `j` is
//! `jal x0, off`. Encodings below are the assembler's, byte-swapped to
//! little-endian.

/// The initcode image: 8 instructions + the message, 49 bytes, copied
/// by `uvm::init` into a fresh page at user va 0.
pub static INITCODE: [u8; 49] = [
    // 0x01000893  li a7, 16   (SYS_write)
    0x93, 0x08, 0x00, 0x01,
    // 0x00100513  li a0, 1    (fd 1)
    0x13, 0x05, 0x10, 0x00,
    // 0x02000593  li a1, 32   (&msg)
    0x93, 0x05, 0x00, 0x02,
    // 0x01000613  li a2, 16   (len)
    0x13, 0x06, 0x00, 0x01,
    // 0x00000073  ecall
    0x73, 0x00, 0x00, 0x00,
    // 0x00b00893  li a7, 11   (SYS_getpid)
    0x93, 0x08, 0xb0, 0x00,
    // 0x00000073  ecall
    0x73, 0x00, 0x00, 0x00,
    // 0xfe5ff06f  j start     (offset -28)
    0x6f, 0xf0, 0x5f, 0xfe,
    // "hello from user\n\0"
    b'h', b'e', b'l', b'l', b'o', b' ', b'f', b'r', b'o', b'm', b' ', b'u', b's', b'e', b'r',
    b'\n', 0,
];
