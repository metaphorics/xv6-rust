OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(pvh_entry)

PHDRS
{
  note PT_NOTE FLAGS(4);
  text PT_LOAD FLAGS(5);
  data PT_LOAD FLAGS(6);
}

SECTIONS
{
  . = 0x100000;
  .note : ALIGN(4) { KEEP(*(.note.Xen)) } :note
  . = ALIGN(0x1000);
  PROVIDE(_kernel_start = .);
  .text : {
    KEEP(*(.text.boot))
    . = ALIGN(0x1000);
    PROVIDE(x86_trampoline_start = .);
    KEEP(*(.text.trap))
    PROVIDE(x86_trampoline_end = .);
    ASSERT(x86_trampoline_end - x86_trampoline_start <= 0x1000,
           "x86 trap trampoline exceeds one page")
    . = ALIGN(0x1000);
    *(.text .text.*)
    . = ALIGN(0x1000);
    PROVIDE(etext = .);
  } :text
  .rodata : ALIGN(0x1000) { *(.rodata .rodata.*) *(.eh_frame .eh_frame.*) } :data
  .data : ALIGN(0x1000) { *(.data .data.*) *(.got .got.*) } :data
  .bss : ALIGN(0x1000) {
    *(.bss.boot)
    *(.bss .bss.*)
    *(COMMON)
    . = ALIGN(0x1000);
    PROVIDE(end = .);
  } :data
  /DISCARD/ : { *(.comment) *(.note.gnu*) }
}
