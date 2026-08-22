//! System-wide tunable limits (`kernel/param.h`). Each constant lands
//! here together with the subsystem that consumes it.

/// Maximum number of CPUs (`param.h:2`); sizes the per-hart boot stacks.
pub const NCPU: usize = 8;

/// Maximum number of processes (`param.h:1`); sizes the proc table and
/// the per-process kernel-stack mappings.
pub const NPROC: usize = 64;
