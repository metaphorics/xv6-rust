//! PVH physical entry and the 32-bit to long-mode climb.

use core::arch::global_asm;

global_asm!(
    r#"
    .section .note.Xen,"a",@note
    .balign 4
    .long 4, 4, 18
    .asciz "Xen"
    .balign 4
    .long pvh_entry

    .section .text.boot,"ax"
    .code32
    .globl pvh_entry
pvh_entry:
    cli
    cld

    movl $boot_pml4, %edi
    xorl %eax, %eax
    movl $(4096 * 3 / 4), %ecx
    rep stosl

    movl $boot_pdpt, %eax
    orl $3, %eax
    movl %eax, boot_pml4
    movl $boot_pd, %eax
    orl $3, %eax
    movl %eax, boot_pdpt

    xorl %ecx, %ecx
1:
    movl %ecx, %eax
    shll $21, %eax
    orl $0x83, %eax
    movl %eax, boot_pd(,%ecx,8)
    movl $0, boot_pd+4(,%ecx,8)
    incl %ecx
    cmpl $512, %ecx
    jne 1b

    lgdt boot_gdt_desc
    movl %cr4, %eax
    orl $0x20, %eax
    movl %eax, %cr4
    movl $boot_pml4, %eax
    movl %eax, %cr3

    movl $0xc0000080, %ecx
    rdmsr
    orl $0x900, %eax
    wrmsr

    movl %cr0, %eax
    orl $0x80010000, %eax
    movl %eax, %cr0
    ljmp $0x08, $long_entry

    .code64
long_entry:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    xorw %ax, %ax
    movw %ax, %fs
    movw %ax, %gs
    movq $boot_stack_top, %rsp
    xorq %rbp, %rbp

    movq %cr0, %rax
    andq $~4, %rax
    orq $2, %rax
    movq %rax, %cr0
    movq %cr4, %rax
    orq $0x600, %rax
    movq %rax, %cr4
    fninit

    xorl %edi, %edi
    call x86_rust_entry
2:
    cli
    hlt
    jmp 2b

    .balign 8
boot_gdt:
    .quad 0
    .quad 0x00af9a000000ffff
    .quad 0x00af92000000ffff
boot_gdt_end:
boot_gdt_desc:
    .word boot_gdt_end - boot_gdt - 1
    .long boot_gdt

    .section .bss.boot,"aw",@nobits
    .balign 4096
boot_pml4:
    .skip 4096
boot_pdpt:
    .skip 4096
boot_pd:
    .skip 4096
    .balign 16
boot_stack:
    .skip 65536
boot_stack_top:
"#,
    options(att_syntax)
);

/// First Rust code after the long-mode transition.
#[unsafe(no_mangle)]
extern "C" fn x86_rust_entry(cpu: usize) -> ! {
    crate::main(cpu)
}
