# M6 replay report

Date: 2026-08-23  
Baseline: `d994d6e` (`docs(m5): record filesystem replay`)  
Implementation commit: `bf93c9d` (`feat(m6): boot real init and shell`)

## Delivered

- Added the ELF64 exec loader with checked program-header decoding, segment permissions, stack guard, 16-byte-aligned argument marshalling, process-name update, and atomic image replacement.
- Added `uvm::clear` and page-crossing `copy_instr`; exec failures own their replacement image through an RAII guard whose current `sz` includes the post-stack allocation.
- Added file-descriptor syscall plumbing for exec, open, close, read, write, dup, mknod, fstat, and chdir. File handles and cwd now flow through the existing counted `ProcPrivate` slots across fork and exit.
- Preserved the M5 inode-table locking protocol: inode sleep locks are never acquired while `ITABLE` is held. Failed create paths release both guards and drop/iput the unlinked inode rather than forgetting it.
- Removed the temporary M4 initcode image and the boot-time filesystem self-test. The first process now initializes the filesystem in `forkret` and execs `/init` directly.
- Implemented `ustd`: the `_start` ABI, one-time raw argc/argv conversion into `&[&[u8]]`, RISC-V syscall wrappers, formatting, input helpers, and a locked K&R `sbrk` allocator.
- Added real `init`, the full shell parser/executor, and `echo`, `ls`, and `cat`. The shell tokenizer returns `b'a'` for words, and `cd` accepts a final line without a newline.
- Added `user.ld` with rodata and unwind tables in the RX segment and page-aligned RW data; release profiles strip symbols.
- Rebuilt xtask's user pipeline to compile release binaries, stage `user/_<name>`, inject them into `fs.img`, consume raw console chunks, and wait for `$ ` before every command.

## Verification

`cargo xtask check` passed after the final source change. It covered:

- workspace rustfmt check;
- host clippy with warnings denied;
- kernel dev clippy with warnings denied;
- kernel release build;
- user dev clippy for every bin with warnings denied;
- stripped user release build for every bin;
- all ABI and mkfs host tests (7 passed).

Real QEMU shell smoke passed with `cargo xtask run --arch riscv64`:

- observed `init: starting sh` and the raw `$ ` prompt;
- `echo hi` produced the exact `hi` output line;
- `ls` listed `README`, `init`, and `sh` (plus the remaining staged programs and console);
- `cat README` printed `xv6 is a re-implementation of Dennis Ritchie's and Ken Thompson's Unix` as its first line;
- harness ended with `XTASK: shell smoke ok`.

The dedicated `cargo xtask run --arch riscv64 --echo-test` also passed and ended with `XTASK: echo ok`.
