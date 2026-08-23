# M8a replay report

Replayed M8a from restored M7 HEAD `bc67d4e`, after the independently owned xtask harness commit `14f1b89` landed.

## Kernel and ABI

- Added the shared two-argument `sbrk` modes: eager `1`, lazy `2`.
- `sys_sbrk` now allocates eager growth, always deallocates shrinkage, and advances only `p->sz` for valid lazy growth below `TRAPFRAME`.
- User load/store page faults now enter `uvm::fault`; valid absent pages below `p->sz` receive a zeroed writable user mapping, while mapped protection faults and out-of-range addresses remain fatal.
- `copy_in`, `copy_out`, and `copy_instr` now accept `p->sz` and use the same lazy-fault fallback. Every kernel caller was migrated; `copy_out` still rejects read-only text.
- `PageTable::take_next_leaf` walks only present Sv39 branches. `uvm::unmap_range` uses it to tear down sparse ranges, so `lazy_sbrk` exit does not probe the 67,108,862 possible pages below the final break.

## Usertests

- Added `user/src/bin/usertests/{main.rs,tests_core.rs,tests_am.rs,tests_nz.rs}`.
- The registry matches the C reference exactly and in order: 67 quick tests plus 6 slow tests.
- The driver preserves `-q`, `-c`, `-C`, one-test selection, `countfree` leak bracketing, per-test fork/wait isolation, and the reference output strings used by the harness.
- Ported the 29 core copy, VM, sbrk, fault, and partial-write tests. A-M and N-Z functions are explicit fail-fast interim bodies for the parallel porters.
- User arguments include `argv[0]`: no arguments means `args.len() == 1`, and flags/test names are read from `args[1]`.

## Bound corrections

- `copyinstr2` keeps its 4097-byte exec argument on the heap rather than on the one-page user stack.
- `copyinstr3`, `sbrklast`, `lazy_copyinstr`, and `partial_write` use the C `% != 0` alignment predicate (`!is_multiple_of`).
- `lazy_sbrk` checks the return from every 1 GiB growth, including the first.
- The 12 KiB `BUFSZ` scratch area is static storage behind a safe lock guard, not a stack allocation or unguarded `static mut`.

## Commits

- `14b59cc feat(kernel): add lazy sbrk and vmfault path`
- `fc23bd4 feat(user): add usertests driver and core VM tests`

## Verification

`cargo xtask check` passed after the implementation and formatting. That gate ran:

- rustfmt check;
- host clippy;
- kernel dev clippy and release build;
- user dev clippy and release build, including the 73-entry usertests registry;
- all host unit and doc tests (7 tests passed).

The assignment explicitly deferred QEMU, so no guest boot or usertests execution was run for M8a.
