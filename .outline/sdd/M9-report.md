# M9 — x86_64 adapter report

## Result

M9 boots the existing xv6-rust kernel and userland on QEMU 10.2.1 `q35` with `-smp 1`. The x86_64 run uses the same filesystem, process, syscall, pipe, and shell core as riscv64.

## Boot-path probe

A minimal ELF64 image containing a Xen `XEN_ELFNOTE_PHYS32_ENTRY` note and a 32-bit COM1 `outb` stub was linked at 1 MiB and passed directly to:

```text
qemu-system-x86_64 -machine q35 -m 128M -smp 1 -nographic -kernel <probe.elf>
```

QEMU 10.2.1 reached the physical entry and emitted `P` on COM1. The PVH direct-boot path was therefore selected; the multiboot1 contingency was not needed.

The production entry contains the same note, creates temporary PML4/PDPT/2 MiB identity mappings, enables CR4.PAE, EFER.LME/NXE, CR3, CR0.PG/WP, far-jumps through a 64-bit GDT descriptor, enables the x86_64 FPU/SSE control bits, installs a boot stack, and enters Rust.

## Adapter delivered

- Four-level page tables with 4 KiB user leaves, 2 MiB supervisor identity leaves, per-leaf user/write/execute permissions, hardware NX, CR3 activation, sparse user-leaf traversal, and process kernel-stack mappings.
- UP CPU and interrupt primitives, callee-saved `Context` switching (`rsp`, `rbp`, `rbx`, `r12`–`r15`), and x86_64 `TrapFrame` syscall/exec accessors.
- Runtime GDT and 64-bit TSS with `rsp0` updated before every user return.
- IDT exception gates, q35 IRQ gates, and DPL3 `int 0x80`; common assembly entry switches from a process CR3 to the kernel CR3 before Rust trap handling and returns with `iretq`.
- Local APIC periodic timer at approximately 10 ms and I/O APIC routes for COM1 IRQ4 and q35 virtio INTx. q35 presents the bus-0 virtio function at slot 3/pin A, whose routed GSI is 23.
- COM1 16550 port-I/O implementation behind the existing UART driver.
- Bus-0 PCI scan for `1af4:1042`, modern virtio vendor-capability discovery, VERSION_1 negotiation, queue-0 common configuration, notify doorbell, ISR acknowledgement, bus mastering, and INTx routing. The existing shared virtqueue and block-request core is unchanged apart from selecting the architecture transport.
- x86_64 user linker script and `int 0x80` syscall ABI (`rax` number/result; `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` arguments).
- xtask target selection, dual-target check gate, and QEMU q35 `-smp 1` launch with a modern-only `virtio-blk-pci` disk.

## Arch-neutral seam changes

Shared code changes are interface-neutral only: architecture-selected trap modules, generic page-table helpers (`MAXVA`, PTE physical address and write permission), architecture constructors for initial contexts and exec register state, architecture-provided UART register access, and architecture-selected virtio transport. The successful-exec path now explicitly installs `argc` in the architecture's entry-argument register; this preserves riscv64 behavior and supplies x86_64 `rdi` without changing syscall semantics.

## Verification

`cargo xtask check` completed successfully with formatting, host clippy, riscv64 kernel/user clippy and release builds, x86_64 kernel/user clippy and release builds, and host `abi`/`mkfs` tests.

`cargo xtask run --arch x86_64` completed the attached-filesystem M7 shell smoke on q35/smp1:

- booted to `init: starting sh` and `$ `;
- `echo hi` returned `hi`;
- `ls` listed `README`, `init`, `sh`, and the staged utilities;
- `cat README` matched the reference first line;
- `ls | wc` produced `23 92 576`;
- created `d/f`, read `x`, refused removal of non-empty `d`, then removed the file and directory and confirmed `d` no longer existed.
