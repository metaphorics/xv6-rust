# M7 replay report

Date: 2026-08-23  
Baseline: `abe47a5` (`docs(m6): record shell replay`)  
Implementation commit: `d9d19e2` (`feat(m7): add pipes and complete userland`)

## Delivered

- Added a safe fixed pipe pool with read and write endpoints owned by `File::Pipe`. Pipe conditions stay under the pool spinlock, and `proc::sleep` atomically hands off that condition lock before sleeping so wakeups cannot be lost.
- Added full pipe allocation and descriptor unwind. A failed file-slot allocation, second descriptor allocation, or user `copy_out` closes each endpoint and removes each installed descriptor exactly once.
- Completed the syscall dispatch surface with `pipe`, `link`, `unlink`, `mkdir`, and `sync`; retained the existing `chdir` implementation.
- Ported link-count rules from the C reference: hard links reject directories and `i16::MAX`, new directories create `.` and `..`, parent links account for `..`, `.` and `..` cannot be unlinked, and non-empty directories are refused.
- Preserved `create` failure ownership: an incompletely linked inode is marked unlinked, both inode guards are released, and ordinary `Inode::drop` performs the single reclaim.
- Added commit-sequence waiting for `sync`, including the already-clean fast path and a sleep until the operation active at the call boundary commits.
- Added the fourteen M7 programs: `grep`, `kill`, `ln`, `mkdir`, `rm`, `wc`, `zombie`, `stressfs`, `sync`, `logstress`, `forphan`, `dorphan`, `grind`, and `forktest`.
- Kept every user entry point on the safe `fn(&[&[u8]]) -> i32` argument boundary and added shared `ustd` wrappers for the remaining calls plus `stat`.
- Applied the known-good utility corrections: `atoi` handles empty and sign-only input without indexing past the slice, `ls` pads short names but leaves names at least `DIRSIZ` bytes untruncated, `logstress` owns a full 2000-byte buffer, and `grind` passes its RNG state as `&mut u64`.
- Extended xtask staging to all 19 M7 binaries and added the complete shell sequence. The pipeline expects the staged image-file count plus `console`, `.`, and `..`.

## Verification

`cargo xtask check` passed after the final source change. It covered:

- workspace rustfmt check;
- host clippy with warnings denied;
- kernel dev clippy with warnings denied;
- kernel release build;
- user dev clippy for every bin with warnings denied;
- user release build for every bin;
- all ABI and mkfs host tests (7 passed).

The real scripted QEMU session passed with `cargo xtask run --arch riscv64`:

- `ls | wc` printed `23 92 570`, whose first field is 20 staged image files plus `console`, `.`, and `..`;
- `mkdir d` succeeded;
- `echo x > d/f` followed by `cat d/f` printed `x`;
- `rm d` printed `rm: d failed to delete` while the directory was non-empty;
- `rm d/f` and `rm d` succeeded;
- `ls d` printed `ls: cannot open d`, proving the directory was removed;
- the harness ended with `XTASK: M7 shell smoke ok`.

The dedicated `cargo xtask run --arch riscv64 --echo-test` also passed and ended with `XTASK: echo ok`.
