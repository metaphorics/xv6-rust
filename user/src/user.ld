OUTPUT_ARCH(riscv)
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
    *(.srodata .srodata.*)
    *(.rodata .rodata.*)
    . = ALIGN(16);
    *(.eh_frame .eh_frame.*)
  } :text

  . = ALIGN(0x1000);
  .data : {
    . = ALIGN(16);
    *(.sdata .sdata.*)
    *(.data .data.*)
    *(.got .got.*)
  } :data

  .bss : {
    . = ALIGN(16);
    *(.sbss .sbss.*)
    *(.bss .bss.*)
    *(COMMON)
  } :data

  PROVIDE(end = .);
  /DISCARD/ : { *(.comment .note .note.*) }
}
