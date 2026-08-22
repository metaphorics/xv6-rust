//! Architecture seam.
//!
//! Core kernel code reaches hardware only through `arch::*`. The riscv64
//! adapter (QEMU `virt`) lands first; the x86_64 adapter plugs into the
//! same seam in a later milestone.

#[cfg(target_arch = "riscv64")]
pub mod riscv64;

#[cfg(target_arch = "riscv64")]
pub use riscv64::{cpu_id, intr_get, intr_off, intr_on};

#[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
compile_error!("unsupported target architecture (expected riscv64 or x86_64)");
