# AGENTS.md: xv6-rust

## Project

Pure-Rust rewrite of xv6 targeting riscv64 (QEMU `virt`) first and x86_64
(QEMU `q35`) second, behind one small `arch` seam. Kernel, user programs,
`mkfs`, and the `xtask` harness are all Rust with zero external crates; the
only assembly lives in `global_asm!`/`naked_asm!` blocks where the ISA
demands it.

## Commands

- `cargo xtask check`: rustfmt, clippy `-D warnings` (host members plus
  kernel/user on their riscv64gc target), host unit tests. Must be green
  before every commit.
- `cargo xtask run --arch <riscv64|x86_64>`: build and boot in QEMU.
- `cargo xtask test --arch <riscv64|x86_64>`: usertests + crash-recovery
  suite.
- `cargo test`: host-only unit tests (default workspace members).

## Layout

- `abi/`: `no_std`, `forbid(unsafe_code)`; syscall numbers, `Stat`, fcntl
  flags, and on-disk fs types shared by kernel, user, and mkfs.
- `kernel/`: `no_std` kernel bin; default target riscv64gc-unknown-none-elf
  (see `kernel/.cargo/config.toml`).
- `user/`: `no_std` ustd runtime lib; one bin per user program.
- `mkfs/`: host tool that builds `fs.img` from `abi` types.
- `xtask/`: host orchestration bin (`check`, later `run`/`test`).

## Invariants

- `unsafe` is a budget, not a tool: safe construction by default (RAII
  guards, move semantics, explicit codecs, token types). The permitted
  surface is only what stable Rust has no safe abstraction for
  (`global_asm!`/`naked_asm!` bodies, the panic handler, volatile MMIO/PIO,
  linker-defined symbols, the page-table hardware format, `ctx_switch`, and
  the trampoline/trapframe hand-off), each in a minimal block with a
  `// SAFETY:` comment.
- Lock-ordering rules live in `kernel/src/proc/`; consult them before
  touching scheduler/sleep/wakeup paths.
- Zero external crates in every workspace member; no nightly, no build-std,
  no custom target JSON.

## Per-task goals

Success criteria for the current task live in
`.agent-tasks/<task-id>/GOALS.md`, never in this file.
