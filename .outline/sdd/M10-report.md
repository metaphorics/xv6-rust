# M10 — x86_64 SMP and full parity

Status: PASS

Implementation commit: `14ea097 feat(x86_64): add SMP and full parity`
Base: `1fad3a2 feat(x86_64): add userland and q35 smoke`

## Implementation

- Changed the q35 launch contract to `-smp 3`.
- Added a reserved low-memory AP bootstrap page at `0x7000`. The BSP copies a position-independent 16-bit trampoline there, supplies each AP with a private temporary PML4/PDPT/PD, stack, CPU index, and Rust entry point, then sends INIT and two SIPIs through the LAPIC.
- Added an acquire/release AP-start handshake and retained the kernel-wide boot release barrier. APs climb real mode → protected mode → long mode on private tables, then switch to the published kernel page table before per-CPU initialization.
- Added LAPIC-ID registration and lookup so `cpu_id()` maps QEMU APIC IDs to contiguous CPU slots. The existing static per-CPU state and interrupt-off discipline now serve all three CPUs.
- Made GDT, TSS, LAPIC timer, and IDT initialization per-CPU where required. The BSP owns IOAPIC routing; COM1 and virtio PCI INTx remain routed to it.
- Mapped the trap entry page, IDT, per-CPU GDT/TSS pages, and kernel stacks at supervisor-only upper-half addresses in every x86 process table. User tables no longer identity-map the kernel, so the full shared `MAXVA = 1 << 38` user ABI remains available without colliding with kernel mappings.
- Kept the RISC-V startup path unchanged: all harts wait for the release barrier before activating the published kernel page table.

## Verification

| Command | Result | Observed proof |
|---|---|---|
| `cargo xtask check` | PASS | rustfmt, host clippy/tests, riscv64 kernel/user clippy+release, x86_64 kernel/user clippy+release |
| `cargo xtask run --arch x86_64` | PASS (2.06 s) | `hart 1 running`, `hart 2 running`, shell, console input/output, virtio-backed `ls`/`cat`, pipes, directory and file operations |
| `cargo xtask test --arch x86_64 quick` | PASS (53.84 s) | all quick usertests; `preempt` exercises per-CPU LAPIC timers and scheduler migration; `ALL TESTS PASSED` |
| `cargo xtask test --arch x86_64 full` | PASS (129.70 s, below 600 s) | quick and all six slow tests; `ALL TESTS PASSED` |
| `cargo xtask test --arch x86_64 crash` | PASS (9.08 s) | log recovery, forphan recovery, dorphan recovery, final `XTASK: crash recovery tests ok` |
| `cargo xtask test --arch riscv64 quick` | PASS (59.56 s) | all quick usertests; `ALL TESTS PASSED` |

The x86 shell and test logs show both APs entering their schedulers. The `preempt` test passed under three active LAPIC timers. COM1 carried all harness traffic, while the full filesystem suite and three crash/reboot sequences exercised IOAPIC delivery and virtio-blk PCI I/O under SMP.
