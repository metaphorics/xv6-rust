# M5 replay report

## Scope

Replayed M5 from restored M4 HEAD `7b7730a`: shared on-disk codecs, a host `mkfs`, virtio-MMIO block I/O, buffer cache, write-ahead log, inode/directory/path/file layers, process cwd/ofile ownership, filesystem initialization, the retained M5 filesystem selftest, and `xtask` disk-image attachment.

## On-disk contract and image

- `abi/src/lib.rs:151-177` owns `BSIZE=1024`, `FSMAGIC=0x1020_3040`, `NDIRECT=12`, `NINDIRECT=256`, `MAXFILE=268`, `DIRSIZ=14`, `FSSIZE=2000`, `MAXOPBLOCKS=10`, `LOGBLOCKS=30`, `IPB=16`, and `BPB=8192`. `abi/src/lib.rs:180-188` owns the inode and bitmap block helpers used by both host and kernel.
- `Superblock`, `Dinode`, `Dirent`, and `LogHeader` use explicit little-endian codecs; no on-disk struct overlay or unsafe parsing is used. Unit tests pin round trips, serialized lengths, C-compatible in-memory sizes, and block helpers.
- `mkfs/src/main.rs:28-44` copies the reference layout math: 31 log blocks including the header, inode start 33, bitmap start 46, first data block 47, and 1,953 data blocks. It writes the root `.`/`..` entries, regular inputs, direct and single-indirect blocks, the root-size rounding, and allocation bitmap. The round-trip test reopens the generated image through ABI codecs and verifies the root and file contents.

## Kernel storage stack

- `kernel/src/dev/virtio/mmio.rs:54-113` implements the virtio-MMIO v2 identity checks, status handshake, feature negotiation, queue setup, and 64-bit queue addresses. Queue pages are static, 4 KiB-aligned DMA storage rather than `kalloc` pages; this keeps their physical identity stable without adding allocator lifetime state.
- `kernel/src/dev/virtio/blk.rs:105-204` implements three-descriptor requests, sleeping completion, interrupt acknowledgement, descriptor recycling, and status checks. Both available and used indices advance with `wrapping_add(1)` (`queue.rs:127`, `blk.rs:202`) for C `uint16_t` behavior.
- `kernel/src/fs/bcache.rs` uses an index-based doubly linked LRU rather than raw pointers. Cache identity lookup matches `dev:blockno` regardless of refcount (`bcache.rs:113-125`), preserving a hit after `brelse`.
- `kernel/src/fs/log.rs:57-101` implements transaction reservation and caller-owned RAII `end_op`; `log.rs:166-176` performs the four commit phases. Boot recovery reads the header, installs committed blocks, and clears it.
- `kernel/src/sync/sleeplock.rs:17-57` tracks waiter count and avoids the process-table wakeup scan when uncontended.

## Inode, directory, path, and file ownership

- The inode cache separates itable identity/ref/valid state from per-inode sleeping metadata locks. `iget` writes recycled invalid state only under itable. `Inode::drop` marks a final reference as reclaiming, releases itable before acquiring the sleeping lock or doing disk work, and never starts a nested transaction (`kernel/src/fs/inode.rs:107-180`).
- Direct `bmap` allocation reassigns the outer `addr` before returning it (`kernel/src/fs/inode.rs:431-441`); no shadowing bug remains. The same layer implements allocation/free, direct and indirect mapping, truncate, inode I/O, directory empty-slot reuse, and absolute/relative path traversal.
- `kernel/src/fs/file.rs` supplies counted inode/device files. Final inode close owns exactly one transaction (`file.rs:210-227`). Shared descriptor offsets are loaded and advanced while the inode lock remains held (`file.rs:87-106`, `file.rs:131-161`).
- `ProcPrivate` now carries `cwd` and `ofile`; fork duplicates their counted handles and exit closes them.
- The M4 hand-built initcode still has no way to open `/dev/console`. M5 therefore retains the narrow fd 1/2 direct-console syscall bridge documented at `kernel/src/syscall.rs:60-66`; the real file/device route is implemented and exercised by the M5 selftest, while M6 removes the bridge when init and file syscalls land. Consequently M6's `sysfile::create_fail` path does not exist in this milestone; no failed-create leak or `mem::forget` workaround was introduced.

## Initialization and selftest

- Hart 0 initializes virtio before `user_init`; filesystem initialization runs once from `forkret`, where disk I/O may sleep, matching the reference's process-context requirement.
- `kernel/src/fs/mod.rs:36-126` retains the M5 selftest. It exercises cached superblock I/O, log transactions, inode allocation and truncate, directory links, absolute path and parent lookup, direct-block file writes crossing a block boundary, file-table read/write/stat, and the device-file dispatch. Success prints `fs selftest: all layers passed`.
- `xtask/src/main.rs:173-184` rebuilds `fs.img` from the reference README for every run. `xtask/src/main.rs:237-265` attaches it with the reference `-drive`, `virtio-blk-device`, bus, and `virtio-mmio.force-legacy=false` options. Normal run requires the filesystem selftest marker followed by two `hello from user` lines; echo mode requires the marker before its console liveness test.
- Existing `KSTACK_PAGES=4` was not changed.

## Commits

1. `5b58c8a feat(abi,mkfs): add on-disk codecs and image builder`
2. `3b1a03a feat(kernel): add virtio storage cache and log`
3. `6797a88 feat(kernel): add inode path and file layers`

## Verification

- `cargo xtask check`: passed. Rustfmt, host/kernel/user Clippy with `-D warnings`, and ABI/mkfs tests were green; ABI ran 6 tests and mkfs ran its image round-trip test.
- `cd kernel && cargo build`: passed.
- `cd kernel && cargo build --release`: passed.
- `cd kernel && cargo clippy -- -D warnings`: passed.
- `cargo xtask run --arch riscv64`: passed with attached `fs.img`; serial output printed `fs selftest: all layers passed`, then two `hello from user` lines, then `XTASK: ok`.
- `cargo xtask run --arch riscv64 --echo-test`: passed with the same filesystem marker, repeated user output, ordered `ABC` and `DEF` echo sentinels across the timer interval, and `XTASK: echo ok`.
