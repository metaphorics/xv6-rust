/* Linker script for the riscv64 kernel: base 0x80000000, ENTRY(_entry),
 * sections per .references/xv6-riscv/kernel/kernel.ld minus the
 * trampoline capture (M4). kernel/.cargo/config.toml passes this file to
 * the linker with -T; keep the relative path in sync. */
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
        /* The trampoline page is captured here once trampoline.S exists
         * (M4): `_trampoline = .; *(trampsec)` followed by a one-page
         * ASSERT, as kernel.ld:16-19 does. */
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
