//! System-wide tunable limits (`kernel/param.h`). Each constant lands
//! here together with the subsystem that consumes it.

/// Maximum number of CPUs (`param.h:2`); sizes the per-hart boot stacks.
pub const NCPU: usize = 8;

/// Maximum number of processes (`param.h:1`); sizes the proc table and
/// the per-process kernel-stack mappings.
pub const NPROC: usize = 64;

/// Buffer-cache entries (`param.h:11`).
pub const NBUF: usize = 30;
/// In-memory inode-cache entries (`param.h:5`).
pub const NINODE: usize = 50;
/// System-wide open-file entries (`param.h:3`).
pub const NFILE: usize = 100;
/// Per-process open-file descriptors (`param.h:4`).
pub const NOFILE: usize = 16;
