# M8b replay report

Replayed the A-M usertests shard from restored M8a.

## A-M usertests

- Replaced all 26 interim bodies in `user/src/bin/usertests/tests_am.rs` with faithful ports of the corresponding xv6-riscv C tests.
- Preserved the registry function names, reference failure strings, loop bounds, generated path bytes, fork/wait behavior, and cleanup operations.
- `badwrite` passes the reference invalid address `0xffffffffff` through the raw write syscall ABI without constructing an invalid Rust reference.
- The C `BUFSZ` scratch array is a zero-initialized module static in `.bss.usertests_am`. Its safe closure accessor serializes the only mutable access with an atomic lock; no test places the 12 KiB buffer on the one-page user stack.
- Kept the change isolated from `main.rs`, `tests_core.rs`, and `tests_nz.rs`.

## Verification

The required checks passed:

- `cd user && cargo build --bins`
- `cd user && cargo build --release --bins`
- `cd user && cargo clippy --bins -- -D warnings`
- `cargo fmt --all -- --check`

A full guest run was attempted with `cargo xtask test --arch riscv64 full`. It booted xv6 and passed the six core tests plus N-Z `truncate1`, then stopped before reaching this shard because the independently owned N-Z `truncate2` test reported `write returned 0, expected -1`. No N-Z source was changed. A follow-up attempt to launch QEMU for standalone A-M commands was blocked by the harness daemon broker socket being unavailable.
