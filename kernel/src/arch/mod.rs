//! Architecture seam.
//!
//! Core kernel code reaches hardware only through `arch::*`. The riscv64
//! adapter (QEMU `virt`) lands first; the x86_64 adapter plugs into the
//! same seam in a later milestone.

/// Bytes per page (`PGSIZE`, riscv.h:389); 4 KiB on both adapters.
pub const PAGE_SIZE: usize = 4096;

/// Usable pages in each process's kernel stack. Unmapped guard pages
/// separate adjacent stacks and the first stack from the trampoline.
pub const KSTACK_PAGES: usize = 4;

#[cfg(target_arch = "riscv64")]
pub mod riscv64;

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "riscv64")]
pub use riscv64::swtch::{Context, switch};

#[cfg(target_arch = "x86_64")]
pub use x86_64::swtch::{Context, switch};

#[cfg(target_arch = "riscv64")]
pub use riscv64::trapframe::TrapFrame;

#[cfg(target_arch = "x86_64")]
pub use x86_64::trapframe::TrapFrame;

#[cfg(target_arch = "riscv64")]
pub use riscv64::vm::{
    MAXVA, PageTable, Perm, TRAMPOLINE, TRAPFRAME, activate_kernel_table, init_kernel_table,
    kstack, prepare_user_table, pte_addr, pte_writable, trampoline_addr,
};

#[cfg(target_arch = "x86_64")]
pub use x86_64::vm::{
    MAXVA, PageTable, Perm, TRAMPOLINE, TRAPFRAME, activate_kernel_table, init_kernel_table,
    kstack, prepare_user_table, pte_addr, pte_writable, trampoline_addr,
};

#[cfg(target_arch = "riscv64")]
pub use riscv64::{
    PHYSTOP, cpu_id, intr_get, intr_off, intr_on, start_other_cpus, uart_read, uart_write,
    wait_for_interrupt,
};

#[cfg(target_arch = "x86_64")]
pub use x86_64::{
    PHYSTOP, cpu_id, intr_get, intr_off, intr_on, start_other_cpus, uart_read, uart_write,
    wait_for_interrupt,
};

#[cfg(target_arch = "riscv64")]
pub use riscv64::intr::{
    init as init_interrupt_controller, init_hart as init_interrupt_controller_hart,
};

#[cfg(target_arch = "x86_64")]
pub use x86_64::intr::{
    init as init_interrupt_controller, init_hart as init_interrupt_controller_hart,
};

#[cfg(target_arch = "riscv64")]
pub(crate) use riscv64::virtio as virtio_transport;

#[cfg(target_arch = "x86_64")]
pub(crate) use x86_64::pci as virtio_transport;

#[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
compile_error!("unsupported target architecture (expected riscv64 or x86_64)");
