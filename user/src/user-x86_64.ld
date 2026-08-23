OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(5);
  data PT_LOAD FLAGS(6);
}

SECTIONS
{
  . = 0;
  .text : {
    *(.text .text.*)
    . = ALIGN(16);
    *(.rodata .rodata.*)
    . = ALIGN(16);
    *(.eh_frame .eh_frame.*)
  } :text
  . = ALIGN(0x1000);
  .data : {
    *(.data .data.*)
    *(.got .got.*)
  } :data
  .bss : {
    *(.bss .bss.*)
    *(COMMON)
  } :data
  PROVIDE(end = .);
  /DISCARD/ : { *(.comment .note .note.*) }
}
