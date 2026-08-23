# M8c replay report

Replayed the N-Z half of xv6-riscv `usertests.c` from restored M8a HEAD `92bee92`.

## Usertests N-Z

- Replaced every interim body in `user/src/bin/usertests/tests_nz.rs` with faithful Rust ports for all 18 registry slots: `truncate1`, `truncate2`, `truncate3`, `openiput`, `opentest`, `writetest`, `writebig`, `pipe1`, `preempt`, `reparent`, `twochildren`, `reparent2`, `sharedfd`, `unlinkread`, `subdir`, `rmdot`, `unlinkcwd`, and `outofinodes`.
- Preserved the C reference's paths, output strings, loop bounds, syscall order, process exits, wait statuses, descriptor offsets, and intentionally non-fatal returns in `pipe1`, `preempt`, and `unlinkcwd`.
- Kept the C global `buf` equivalent in static zero-initialized storage. A lock guard provides safe Rust access to the 12 KiB buffer without placing it on xv6's one-page user stack or exposing an unguarded `static mut`.
- Used the shared ABI constants `BSIZE`, `MAXFILE`, and `MAXOPBLOCKS`, so file-size and scratch-buffer bounds remain tied to the kernel ABI.
- Made no changes to the test registry, core tests, or the independently owned A-M shard.

## Commit

- `test(user): port usertests N-Z`

## Verification

The required user checks passed with the unavailable workstation `sccache` wrapper disabled:

- `cargo fmt --all --check`
- `cargo build --bins` in `user/`
- `cargo build --release --bins` in `user/`
- `cargo clippy --bins -- -D warnings` in `user/`

A guest quick-suite smoke reached the new shard and confirmed `truncate1: OK`. It then stopped at the faithful `truncate2` assertion because the current kernel returned `0` for the beyond-EOF write where xv6-riscv returns `-1`:

```text
test truncate1: OK
test truncate2: truncate2: write returned 0, expected -1
FAILED
```

The test port deliberately retains the C result check; accepting `0` would hide the kernel behavior difference. The remaining N-Z slots could not run in that quick-suite invocation after its fail-fast stop.
