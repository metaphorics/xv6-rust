//! Architecture seam.
//!
//! Core kernel code reaches hardware only through `arch::*`. The riscv64
//! adapter (QEMU `virt`) lands first; the x86_64 adapter plugs into the
//! same seam in a later milestone.

/// Bytes per page (`PGSIZE`, riscv.h:389); 4 KiB on both adapters.
pub const PAGE_SIZE: usize = 4096;

#[cfg(target_arch = "riscv64")]
pub mod riscv64;

#[cfg(target_arch = "riscv64")]
pub use riscv64::swtch::{switch, Context};

#[cfg(target_arch = "riscv64")]
pub use riscv64::trapframe::TrapFrame;

#[cfg(target_arch = "riscv64")]
pub use riscv64::vm::{activate, kstack, PageTable, Perm, TRAMPOLINE, TRAPFRAME};

#[cfg(target_arch = "riscv64")]
pub use riscv64::{cpu_id, intr_get, intr_off, intr_on, wait_for_interrupt};

#[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
compile_error!("unsupported target architecture (expected riscv64 or x86_64)");
