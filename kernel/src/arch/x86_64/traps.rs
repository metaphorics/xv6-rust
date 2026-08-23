//! IDT gates, common trap entry, and `iretq` user return.

use core::cell::UnsafeCell;

use super::gdt;
use super::vm::{TRAP_ENTRY_VA, TRAPFRAME};
use crate::arch::PAGE_SIZE;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        attributes: 0,
        offset_mid: 0,
        offset_high: 0,
        reserved: 0,
    };

    fn gate(address: u64, dpl3: bool) -> Self {
        Self {
            offset_low: address as u16,
            selector: gdt::KERNEL_CODE,
            ist: 0,
            attributes: if dpl3 { 0xee } else { 0x8e },
            offset_mid: (address >> 16) as u16,
            offset_high: (address >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

#[repr(align(4096))]
struct Shared<T>(UnsafeCell<T>);
// SAFETY: the IDT is initialized once before interrupts are enabled.
unsafe impl<T> Sync for Shared<T> {}

static IDT: Shared<[IdtEntry; 256]> = Shared(UnsafeCell::new([IdtEntry::MISSING; 256]));
pub const IDT_VA: u64 = super::vm::KERNEL_HIGH_BASE + 2 * 1024 * 1024;

pub fn idt_addr() -> u64 {
    IDT.0.get() as u64
}

unsafe extern "C" {
    static x86_vector_table: [u64; 57];
    fn x86_userret(trapframe_pa: u64, cr3: u64) -> !;
    fn x86_common_entry();
}

pub fn init(cpu: usize) {
    if cpu == 0 {
        // SAFETY: the BSP initializes the static IDT before it starts the APs.
        let idt = unsafe { &mut *IDT.0.get() };
        for (vector, address) in (0usize..=55).zip(
            // SAFETY: assembly emits 57 address-sized entries.
            unsafe { x86_vector_table[..56].iter().copied() },
        ) {
            idt[vector] = IdtEntry::gate(trampoline_address(address), false);
        }
        // SAFETY: the final table element is vector 128's stub.
        idt[128] = IdtEntry::gate(trampoline_address(unsafe { x86_vector_table[56] }), true);
    }
    let idtr = Idtr {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: IDT_VA,
    };
    // SAFETY: IDT storage is static and every installed gate names kernel code.
    unsafe { core::arch::asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack)) };
}

pub fn entry_addr() -> u64 {
    x86_common_entry as *const () as usize as u64
}

fn trampoline_address(address: u64) -> u64 {
    let source_page = entry_addr() & !((PAGE_SIZE as u64) - 1);
    let offset = address
        .checked_sub(source_page)
        .expect("trap entry before trampoline");
    assert!(offset < PAGE_SIZE as u64, "trap entry outside trampoline");
    TRAP_ENTRY_VA + offset
}

pub fn return_to_user(trapframe_pa: u64, cr3: u64) -> ! {
    let entry = trampoline_address(x86_userret as *const () as usize as u64);
    // SAFETY: `trapframe_pa` is the current process's stable, mapped
    // trapframe page; entry is executable x86_userret, rdi/rsi carry its
    // declared integer arguments, and the routine never returns.
    unsafe {
        core::arch::asm!(
            "jmp rax",
            in("rax") entry,
            in("rdi") trapframe_pa,
            in("rsi") cr3,
            options(noreturn)
        )
    }
}

core::arch::global_asm!(
    r#"
    .section .text.trap,"ax"
    .macro X86_ISR vector
    .globl x86_vector_\vector
x86_vector_\vector:
    .if (\vector == 8) || ((\vector >= 10) && (\vector <= 14)) || (\vector == 17) || (\vector == 21) || (\vector == 29) || (\vector == 30)
    .else
    pushq $0
    .endif
    pushq $\vector
    jmp x86_common_entry
    .endm

    .irp vector,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,128
    X86_ISR \vector
    .endr
    .globl x86_common_entry
x86_common_entry:
    cld
    pushq %rax
    pushq %rbx
    pushq %rcx
    pushq %rdx
    pushq %rsi
    pushq %rdi
    pushq %rbp
    pushq %r8
    pushq %r9
    pushq %r10
    pushq %r11
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15

    testb $3, 144(%rsp)
    jz 1f
    movq X86_KERNEL_CR3(%rip), %rax
    movq %rax, %cr3
1:
    movq %rsp, %r12
    movq %rsp, %rdi
    andq $-16, %rsp
    movabsq $x86_trap_dispatch, %rax
    call *%rax
    movq %r12, %rsp

    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %r11
    popq %r10
    popq %r9
    popq %r8
    popq %rbp
    popq %rdi
    popq %rsi
    popq %rdx
    popq %rcx
    popq %rbx
    popq %rax
    addq $16, %rsp
    iretq

    .globl x86_userret
x86_userret:
    cli
    movq %rdi, %rbx
    pushq $0x1b
    pushq 144(%rbx)
    movq 136(%rbx), %rax
    orq $0x200, %rax
    pushq %rax
    pushq $0x23
    pushq 128(%rbx)
    movabsq ${trapframe}, %rbx
    movq %rsi, %cr3

    movq 8(%rbx), %r15
    movq 16(%rbx), %r14
    movq 24(%rbx), %r13
    movq 32(%rbx), %r12
    movq 40(%rbx), %r11
    movq 48(%rbx), %r10
    movq 56(%rbx), %r9
    movq 64(%rbx), %r8
    movq 72(%rbx), %rbp
    movq 88(%rbx), %rsi
    movq 96(%rbx), %rdx
    movq 104(%rbx), %rcx
    movq 120(%rbx), %rax
    movq 80(%rbx), %rdi
    movq 112(%rbx), %rbx
    iretq

    .balign 8
    .globl x86_vector_table
x86_vector_table:
    .irp vector,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,128
    .quad x86_vector_\vector
    .endr
    .balign 8
    .globl X86_KERNEL_CR3
X86_KERNEL_CR3:
    .quad 0
"#,
    trapframe = const TRAPFRAME.0,
    options(att_syntax)
);
