//! Virtio-MMIO block device.

pub mod blk;
#[cfg(target_arch = "riscv64")]
mod mmio;
mod queue;

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::pci as transport;
#[cfg(target_arch = "riscv64")]
use mmio as transport;
