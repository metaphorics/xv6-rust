//! PVH physical entry and the 32-bit to long-mode climb.

use core::arch::global_asm;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering, fence};

use crate::params::NCPU;

pub(super) const AP_BOOT_ADDR: usize = 0x7000;
const AP_BOOT_PARAMS: usize = 0x7f00;
const AP_BOOT_VECTOR: u8 = (AP_BOOT_ADDR >> 12) as u8;
const BOOT_CPU_COUNT: usize = 3;
const AP_STACK_SIZE: usize = 64 * 1024;
const _: () = assert!(BOOT_CPU_COUNT <= NCPU);

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

    .section .rodata.ap_boot,"a"
    .code16
    .globl ap_boot_start
ap_boot_start:
    cli
    cld
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw $0x7000, %sp
    lgdtl (0x7000 + ap_gdt_desc - ap_boot_start)

    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    ljmpl $0x18, $(0x7000 + ap_protected - ap_boot_start)

    .code32
ap_protected:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss

    movl %cr4, %eax
    orl $0x20, %eax
    movl %eax, %cr4
    movl 0x7f00, %eax
    movl %eax, %cr3

    movl $0xc0000080, %ecx
    rdmsr
    orl $0x900, %eax
    wrmsr

    movl %cr0, %eax
    orl $0x80010000, %eax
    movl %eax, %cr0
    ljmpl $0x08, $(0x7000 + ap_long - ap_boot_start)

    .code64
ap_long:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    xorw %ax, %ax
    movw %ax, %fs
    movw %ax, %gs
    movq 0x7f08, %rsp
    xorq %rbp, %rbp

    movq %cr0, %rax
    andq $~4, %rax
    orq $2, %rax
    movq %rax, %cr0
    movq %cr4, %rax
    orq $0x600, %rax
    movq %rax, %cr4
    fninit

    movl 0x7f10, %edi
    movq 0x7f18, %rax
    call *%rax
3:
    cli
    hlt
    jmp 3b

    .balign 8
ap_gdt:
    .quad 0
    .quad 0x00af9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00cf9a000000ffff
ap_gdt_end:
ap_gdt_desc:
    .word ap_gdt_end - ap_gdt - 1
    .long 0x7000 + ap_gdt - ap_boot_start
    .globl ap_boot_end
ap_boot_end:
"#,
    options(att_syntax)
);

#[repr(C, align(4096))]
struct Page([u64; 512]);

#[repr(C, align(4096))]
struct ApTables {
    pml4: Page,
    pdpt: Page,
    pd: Page,
}

impl ApTables {
    const ZERO: Self = Self {
        pml4: Page([0; 512]),
        pdpt: Page([0; 512]),
        pd: Page([0; 512]),
    };
}

#[repr(C, align(16))]
struct ApStack([u8; AP_STACK_SIZE]);

struct Shared<T>(UnsafeCell<T>);
// SAFETY: the BSP initializes each row before releasing its corresponding AP.
unsafe impl<T> Sync for Shared<T> {}

static AP_TABLES: [Shared<ApTables>; NCPU] =
    [const { Shared(UnsafeCell::new(ApTables::ZERO)) }; NCPU];
static AP_STACKS: [Shared<ApStack>; NCPU] =
    [const { Shared(UnsafeCell::new(ApStack([0; AP_STACK_SIZE]))) }; NCPU];
static AP_STARTED: [AtomicBool; NCPU] = [const { AtomicBool::new(false) }; NCPU];

unsafe extern "C" {
    static ap_boot_start: u8;
    static ap_boot_end: u8;
}

fn prepare_ap_tables(cpu: usize) -> u64 {
    let tables = AP_TABLES[cpu].0.get();
    // SAFETY: only the BSP writes this CPU's tables, before sending its SIPI.
    unsafe {
        core::ptr::write_bytes(tables.cast::<u8>(), 0, core::mem::size_of::<ApTables>());
        let pml4 = &mut (*tables).pml4.0;
        let pdpt = &mut (*tables).pdpt.0;
        let pd = &mut (*tables).pd.0;
        pml4[0] = pdpt.as_ptr() as u64 | 3;
        pdpt[0] = pd.as_ptr() as u64 | 3;
        for (index, entry) in pd.iter_mut().enumerate() {
            *entry = (index as u64) << 21 | 0x83;
        }
        pml4.as_ptr() as u64
    }
}

fn ap_stack_top(cpu: usize) -> u64 {
    AP_STACKS[cpu].0.get() as u64 + AP_STACK_SIZE as u64
}

pub fn start_aps() {
    super::register_cpu(0, super::intr::local_apic_id());

    let source = (&raw const ap_boot_start).cast::<u8>();
    let size = (&raw const ap_boot_end as usize) - (&raw const ap_boot_start as usize);
    assert!(
        size <= AP_BOOT_PARAMS - AP_BOOT_ADDR,
        "AP bootstrap exceeds low page"
    );
    // SAFETY: the kernel reserves and maps 0x7000; the source is the linked trampoline blob.
    unsafe { core::ptr::copy_nonoverlapping(source, AP_BOOT_ADDR as *mut u8, size) };

    for (cpu, started) in AP_STARTED.iter().enumerate().take(BOOT_CPU_COUNT).skip(1) {
        let apic_id = cpu as u32;
        super::register_cpu(cpu, apic_id);
        let root = prepare_ap_tables(cpu);
        assert!(u32::try_from(root).is_ok(), "AP bootstrap CR3 above 4 GiB");
        started.store(false, Ordering::Relaxed);
        // SAFETY: the parameter block is reserved low memory consumed before this AP reports ready.
        unsafe {
            core::ptr::write_volatile(AP_BOOT_PARAMS as *mut u32, root as u32);
            core::ptr::write_volatile((AP_BOOT_PARAMS + 8) as *mut u64, ap_stack_top(cpu));
            core::ptr::write_volatile((AP_BOOT_PARAMS + 16) as *mut u32, cpu as u32);
            core::ptr::write_volatile(
                (AP_BOOT_PARAMS + 24) as *mut u64,
                x86_ap_rust_entry as *const () as usize as u64,
            );
        }
        fence(Ordering::SeqCst);
        super::intr::start_ap(apic_id, AP_BOOT_VECTOR);
        while !started.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }
}

/// First Rust code after the BSP's long-mode transition.
#[unsafe(no_mangle)]
extern "C" fn x86_rust_entry(cpu: usize) -> ! {
    crate::main(cpu)
}

/// First Rust code after an AP's private long-mode transition.
#[unsafe(no_mangle)]
extern "C" fn x86_ap_rust_entry(cpu: usize) -> ! {
    AP_STARTED[cpu].store(true, Ordering::Release);
    crate::main(cpu)
}
