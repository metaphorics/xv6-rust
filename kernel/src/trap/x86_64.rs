//! x86_64 exception, interrupt, and `int 0x80` handling.

use core::arch::asm;

use crate::arch;
use crate::arch::x86_64::{gdt, intr, traps};
use crate::dev::uart16550;
use crate::mm::uvm;
use crate::proc::{self, CurrentProc};
use crate::sync::SpinLock;
use crate::syscall;

pub(crate) static TICKS: SpinLock<u64> = SpinLock::new(0);

#[repr(C)]
pub struct TrapStack {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
    vector: u64,
    error: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

pub fn init_hart() {
    let rsp: u64;
    // SAFETY: reads the current kernel stack pointer without changing it.
    unsafe { asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags)) };
    let cpu = arch::cpu_id();
    gdt::init(cpu, rsp);
    traps::init(cpu);
}

#[unsafe(no_mangle)]
pub extern "C" fn x86_trap_dispatch(stack: &TrapStack) {
    if stack.cs & 3 == 3 {
        user_trap(stack);
    } else {
        kernel_trap(stack);
    }
}

fn save_user(stack: &TrapStack, p: &CurrentProc) {
    let tf = p.trapframe();
    tf.r15 = stack.r15;
    tf.r14 = stack.r14;
    tf.r13 = stack.r13;
    tf.r12 = stack.r12;
    tf.r11 = stack.r11;
    tf.r10 = stack.r10;
    tf.r9 = stack.r9;
    tf.r8 = stack.r8;
    tf.rbp = stack.rbp;
    tf.rdi = stack.rdi;
    tf.rsi = stack.rsi;
    tf.rdx = stack.rdx;
    tf.rcx = stack.rcx;
    tf.rbx = stack.rbx;
    tf.rax = stack.rax;
    tf.epc = stack.rip;
    tf.rflags = stack.rflags;
    tf.sp = stack.rsp;
}

fn user_trap(stack: &TrapStack) -> ! {
    let p = proc::my_proc().expect("usertrap: no current proc");
    save_user(stack, &p);
    let mut device = 0;

    if stack.vector == 128 {
        if p.killed() {
            proc::exit(-1);
        }
        arch::intr_on();
        syscall::dispatch(&p);
    } else if stack.vector == 14 {
        let address: u64;
        // SAFETY: CR2 is the architectural page-fault linear address.
        unsafe { asm!("mov {}, cr2", out(reg) address, options(nomem, nostack, preserves_flags)) };
        if uvm::fault(p.pagetable_mut(), p.sz(), address, stack.error & 2 == 0).is_none() {
            println!(
                "usertrap(): page fault va={:#x} rip={:#x} pid={}",
                address,
                stack.rip,
                p.pid()
            );
            p.set_killed();
        }
    } else {
        device = device_interrupt(stack.vector);
        if device == 0 {
            println!(
                "usertrap(): unexpected vector {} error={:#x} rip={:#x} pid={}",
                stack.vector,
                stack.error,
                stack.rip,
                p.pid()
            );
            p.set_killed();
        }
    }

    if p.killed() {
        proc::exit(-1);
    }
    if device == 2 {
        proc::yield_now();
    }
    usertrapret(&p)
}

fn kernel_trap(stack: &TrapStack) {
    assert!(!arch::intr_get(), "kerneltrap: interrupts enabled");
    let device = device_interrupt(stack.vector);
    if device == 0 {
        println!(
            "kerneltrap: vector={} error={:#x} rip={:#x}",
            stack.vector, stack.error, stack.rip
        );
        panic!("kerneltrap");
    }
    if device == 2 && proc::my_proc().is_some() {
        proc::yield_now();
    }
}

fn device_interrupt(vector: u64) -> u32 {
    if vector == u64::from(intr::TIMER_VECTOR) {
        clockintr();
        intr::eoi();
        return 2;
    }
    if vector == 32 + u64::from(intr::UART0_IRQ) {
        uart16550::intr();
        intr::eoi();
        return 1;
    }
    if vector == 32 + u64::from(intr::virtio_irq()) {
        crate::dev::virtio::blk::intr();
        intr::eoi();
        return 1;
    }
    if vector == 47 {
        intr::eoi();
        return 1;
    }
    0
}

pub fn clockintr() {
    if arch::cpu_id() == 0 {
        let mut ticks = TICKS.lock();
        *ticks += 1;
        proc::wakeup(TICKS.chan());
    }
}

pub fn usertrapret(p: &CurrentProc) -> ! {
    arch::intr_off();
    let tf = p.trapframe();
    tf.kernel_sp = p.kstack_top();
    gdt::set_rsp0(p.kstack_top());
    traps::return_to_user(tf, p.pagetable().cr3_value())
}
