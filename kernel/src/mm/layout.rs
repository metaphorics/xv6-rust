//! Physical memory layout constants (`kernel/memlayout.h`).

use super::addr::PhysAddr;

/// UART0 (memlayout.h:21).
pub const UART0: PhysAddr = PhysAddr(0x1000_0000);

/// VIRTIO0 (memlayout.h:25).
pub const VIRTIO0: PhysAddr = PhysAddr(0x1000_1000);

/// PLIC (memlayout.h:33).
pub const PLIC: PhysAddr = PhysAddr(0x0c00_0000);

/// PLIC extent, the size `kvmmake` maps (vm.c:36).
pub const PLIC_SIZE: u64 = 0x0400_0000;

/// KERNBASE, where QEMU loads the kernel (memlayout.h:43).
pub const KERNBASE: PhysAddr = PhysAddr(0x8000_0000);

/// PHYSTOP: KERNBASE + 128 MiB — the RAM the kernel uses (memlayout.h:44).
pub const PHYSTOP: PhysAddr = PhysAddr(0x8800_0000);
