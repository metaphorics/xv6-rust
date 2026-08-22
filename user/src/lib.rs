#![no_std]

//! The future `ustd` userland runtime.
//!
//! M6 fills this in: the `_start` entry shim, syscall stubs generated from
//! `abi::Sys`, `print!`/`println!` over `write(1, ..)`, and the first-fit
//! `GlobalAlloc` over `sbrk` (the port of umalloc). For now the crate
//! anchors the user-side build pipeline for the riscv64gc target.
