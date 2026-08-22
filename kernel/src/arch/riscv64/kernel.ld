/* Linker script for the riscv64 kernel: base 0x80000000, ENTRY(_entry),
 * sections per .references/xv6-riscv/kernel/kernel.ld including the
 * one-page trampoline capture in .text. kernel/.cargo/config.toml passes
 * this file to the linker with -T; keep the relative path in sync. */
OUTPUT_ARCH(riscv)
ENTRY(_entry)

SECTIONS
{
    /*
     * Ensure _entry sits at 0x80000000, where qemu -kernel makes every
     * hart start executing (kernel.ld:6-10).
     */
    . = 0x80000000;

    .text : {
        /* entry.rs lands its .text.entry section first, so _entry is the
         * first byte of the image. */
        *(.text.entry)
        *(.text .text.*)
        . = ALIGN(0x1000);
        /* The trampoline must start on a page boundary so it can be
         * mapped at TRAMPOLINE in both the kernel and user page tables
         * (kernel.ld:16-19). */
        _trampoline = .;
        *(trampsec)
        . = ALIGN(0x1000);
        ASSERT(. - _trampoline == 0x1000, "error: trampoline larger than one page");
        PROVIDE(etext = .);
    }

    .rodata : {
        . = ALIGN(16);
        *(.srodata .srodata.*)
        . = ALIGN(16);
        *(.rodata .rodata.*)
    }

    .data : {
        . = ALIGN(16);
        *(.sdata .sdata.*)
        . = ALIGN(16);
        *(.data .data.*)
    }

    .bss : {
        . = ALIGN(16);
        *(.sbss .sbss.*)
        . = ALIGN(16);
        *(.bss .bss.*)
    }

    PROVIDE(end = .);
}
