# M4 stabilization replay report

## Scope

The restored tree started at `7adf0c5` with the prior stabilization worker's uncommitted changes. `.outline/recovery-state.txt` describes the later M8 loss and recovery; this replay deliberately completed only the requested M4 stabilization surface.

## Stabilization checklist

- Corrected the RISC-V user PTE bit to `PTE_U = 1 << 4` and made `walkaddr` reject addresses at or above `MAXVA` before walking.
- Verified the restored M-mode setup already uses the xv6 PMP address `0x3f_ffff_ffff_ffff` and the boot-stack calculation already uses `slli ..., 12` rather than extension-dependent multiplication.
- Enabled interrupts on non-boot harts after their per-hart PLIC setup and before entering the scheduler.
- Made `Cpu::current_slot` branch before subtracting, avoiding eager underflow in `then_some`.
- Made `sched` re-read the current CPU after the context switch before restoring `intena`, matching the two `mycpu()` evaluations in C.
- Replaced UART's fused no-lock sleep with the reference split protocol: `sleep_prepare`, conditional `sleep_commit`, wakeup-side channel clearing, and retry of the same byte after wakeup.
- Kept each initcode instruction's encoding comment on its own byte row so rustfmt cannot associate it with the preceding instruction.
- Removed stable-rustfmt's ignored nightly-only configuration, formatted the workspace with Rust 1.98, and fixed all new `-D warnings` Clippy diagnostics without changing observable contracts.
- Increased usable per-process kernel stacks to `KSTACK_PAGES = 4`; page-table mapping and saved stack tops use all four pages, with unmapped pages separating stacks.
- Updated the QEMU echo smoke to synchronize on M4's first user output and recognize echoed sentinels as an ordered subsequence, since the continuously writing init process can interleave output between echoed characters.

## Verification

All commands ran with `RUSTC_WRAPPER` cleared because the restored workstation no longer has `sccache`.

- `cargo xtask check`: passed; rustfmt, host Clippy, riscv64 kernel Clippy, riscv64 user Clippy, and host tests all green.
- `cd kernel && cargo build`: passed.
- `cd kernel && cargo build --release`: passed.
- `cargo xtask run --arch riscv64 --echo-test`: passed under QEMU 10.2.1. Serial output showed both secondary harts, repeated `hello from user`, echoed `ABC` and `DEF` across the timer-liveness interval, and ended with `XTASK: echo ok`.

The host's installed `qemu-system-misc` package does not include the RISC-V executable; verification used the same-version `qemu-system-riscv` package extracted under `/tmp` and prepended to `PATH` without modifying the repository.
