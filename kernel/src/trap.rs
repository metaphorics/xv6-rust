//! Trap handling (`kernel/trap.c`) — the kernel-side half plus the
//! user-mode entry/return pair. The vectors themselves are assembly
//! ([`arch::riscv64::kernelvec`] and the trampoline's `uservec`).

use crate::arch;
use crate::arch::riscv64::{self, intr, kernelvec};
use crate::dev::uart16550;
use crate::mm::uvm;
use crate::proc::{self, CurrentProc};
use crate::sync::SpinLock;
use crate::syscall;

/// sstatus.SPP: previous mode, 1 = supervisor (riscv.h:44).
const SSTATUS_SPP: usize = 1 << 8;

/// sstatus.SPIE: supervisor interrupt enable after `sret`
/// (riscv.h:48-50).
const SSTATUS_SPIE: usize = 1 << 5;

/// Supervisor external interrupt, the full scause value (trap.c:192).
const SCAUSE_SUP_EXTERNAL: usize = 0x8000_0000_0000_0009;

/// Supervisor timer interrupt, the full scause value (trap.c:213).
const SCAUSE_SUP_TIMER: usize = 0x8000_0000_0000_0005;

/// Environment call from U-mode: a system call (trap.c:49).
const SCAUSE_ECALL_USER: usize = 8;

/// The tick counter and its lock (`tickslock`, `ticks`, trap.c:9-10).
/// A const-constructed `SpinLock` replaces `trapinit`'s `initlock`
/// (trap.c:19-23). `sys_pause`/`sys_uptime` read it through this lock;
/// sleepers use its address as the channel.
pub(crate) static TICKS: SpinLock<u64> = SpinLock::new(0);

/// Set up to take exceptions and traps while in the kernel
/// (`trapinithart`, trap.c:26-30): install the kernel trap vector.
pub fn init_hart() {
    riscv64::w_stvec(kernelvec::addr());
}

/// Handle an interrupt, exception, or system call from user space
/// (`usertrap`, trap.c:37-93). Called from, and returns to, the
/// trampoline: the return value is the user satp, and `uservec`'s
/// `jalr` lands on `userret` with that satp in a0
/// (trampoline.S:97-101).
pub extern "C" fn usertrap() -> u64 {
    let mut which_dev = 0u32;

    assert!(
        riscv64::r_sstatus() & SSTATUS_SPP == 0,
        "usertrap: not from user mode"
    );

    // Send interrupts and exceptions to kerneltrap, since we're now in
    // the kernel (trap.c:45-46).
    riscv64::w_stvec(kernelvec::addr());

    let p = proc::my_proc().expect("usertrap: no current proc");

    // Save the user program counter (trap.c:48-50).
    p.trapframe().epc = riscv64::r_sepc() as u64;

    let scause = riscv64::r_scause();
    if scause == SCAUSE_ECALL_USER {
        // System call (trap.c:52-65).
        if p.killed() {
            proc::exit(-1);
        }
        // sepc points at the ecall; return to the next instruction.
        p.trapframe().advance_pc();
        // An interrupt will change sepc, scause, and sstatus, so enable
        // only now that we're done with those registers.
        arch::intr_on();
        syscall::dispatch(&p);
    } else {
        which_dev = devintr();
        let faulted = which_dev == 0
            && matches!(scause, 13 | 15)
            && uvm::fault(
                p.pagetable_mut(),
                p.sz(),
                riscv64::r_stval() as u64,
                scause == 13,
            )
            .is_some();
        if which_dev == 0 && !faulted {
            println!(
                "usertrap(): unexpected scause {:#x} pid={}",
                scause,
                p.pid()
            );
            println!(
                "            sepc={:#x} stval={:#x}",
                riscv64::r_sepc(),
                riscv64::r_stval()
            );
            p.set_killed();
        }
    }

    if p.killed() {
        proc::exit(-1);
    }

    // Give up the CPU if this is a timer interrupt (trap.c:83-84).
    if which_dev == 2 {
        proc::yield_now();
    }

    // Return to user space; the satp value is trampoline.S's to use.
    usertrapret(&p);
}

/// Set up the trapframe and control registers for a return to user
/// space, then jump through the trampoline (`prepare_return`,
/// trap.c:98-128, plus `forkret`'s tail, proc.c:539-543): interrupts
/// off, `stvec` to `uservec`'s TRAMPOLINE alias, kernel recovery values
/// into the trapframe, `sstatus` for a user-mode `sret`, and `sepc` to
/// the saved user pc. Never returns.
pub fn usertrapret(p: &CurrentProc) -> ! {
    // We're about to switch the destination of traps from kerneltrap to
    // usertrap; a trap from kernel code to usertrap would be a disaster,
    // so turn off interrupts (trap.c:103-105).
    arch::intr_off();

    // Send syscalls, interrupts, and exceptions to uservec in the
    // trampoline (trap.c:107-109).
    riscv64::w_stvec(riscv64::trampoline::uservec_va());

    // Set up trapframe values that uservec will need when the process
    // next traps into the kernel (trap.c:111-118).
    let tf = p.trapframe();
    tf.kernel_satp = riscv64::r_satp() as u64;
    tf.kernel_sp = p.kstack_top();
    let usertrap_entry: extern "C" fn() -> u64 = usertrap;
    tf.kernel_trap = usertrap_entry as usize as u64;
    tf.kernel_hartid = arch::cpu_id() as u64;

    // Set S Previous Privilege mode to User, and enable interrupts in
    // user mode (trap.c:120-126).
    let x = riscv64::r_sstatus() & !SSTATUS_SPP | SSTATUS_SPIE;
    riscv64::w_sstatus(x);

    // Set S Exception Program Counter to the saved user pc
    // (trap.c:128-129).
    riscv64::w_sepc(tf.epc as usize);

    // Enter the trampoline with the user satp in a0 (proc.c:541-543).
    riscv64::trampoline::user_ret(p.pagetable().satp_value())
}

/// Interrupts and exceptions from kernel code arrive here through
/// `kernelvec`, on whatever the current kernel stack is (`kerneltrap`,
/// trap.c:134-164).
pub extern "C" fn kerneltrap() {
    let sepc = riscv64::r_sepc();
    let sstatus = riscv64::r_sstatus();
    let scause = riscv64::r_scause();

    assert!(
        sstatus & SSTATUS_SPP != 0,
        "kerneltrap: not from supervisor mode"
    );
    assert!(!arch::intr_get(), "kerneltrap: interrupts enabled");

    let which_dev = devintr();
    if which_dev == 0 {
        // Interrupt or trap from an unknown source (trap.c:150-153).
        println!(
            "scause={:#x} sepc={:#x} stval={:#x}",
            scause,
            riscv64::r_sepc(),
            riscv64::r_stval()
        );
        panic!("kerneltrap");
    }

    // Give up the CPU if this is a timer interrupt (trap.c:156-158).
    if which_dev == 2 && proc::my_proc().is_some() {
        proc::yield_now();
    }

    // The yield may have caused traps to occur, so restore the trap
    // registers for kernelvec's sret (trap.c:160-163).
    riscv64::w_sepc(sepc);
    riscv64::w_sstatus(sstatus);
}

/// Timer tick: bump the counter on hart 0 and rearm the sstc comparison
/// (`clockintr`, trap.c:166-180). Every hart rearms its own `stimecmp` —
/// that write also clears the interrupt request.
pub fn clockintr() {
    if arch::cpu_id() == 0 {
        let mut ticks = TICKS.lock();
        *ticks += 1;
        // Wake sleepers in sys_pause (trap.c:172). Waking while holding
        // tickslock is the C order: condition lock, then each p->lock.
        proc::wakeup(TICKS.chan());
    }

    // Ask for the next timer interrupt; this also clears the interrupt
    // request (trap.c:176-179).
    riscv64::w_stimecmp(riscv64::r_time() + riscv64::TIMER_INTERVAL);
}

/// Check if it's an external interrupt or software interrupt, and handle
/// it. Returns 2 if timer interrupt, 1 if other device, 0 if not
/// recognized (`devintr`, trap.c:187-220).
fn devintr() -> u32 {
    let scause = riscv64::r_scause();

    if scause == SCAUSE_SUP_EXTERNAL {
        // This is a supervisor external interrupt, via PLIC (trap.c:193).
        let hart = arch::cpu_id();

        // irq indicates which device interrupted (trap.c:196).
        let irq = intr::claim(hart);
        match irq {
            Some(intr::UART0_IRQ) => uart16550::intr(),
            Some(intr::VIRTIO0_IRQ) => crate::dev::virtio::blk::intr(),
            Some(irq) => println!("unexpected interrupt irq={}", irq),
            None => {}
        }

        // The PLIC allows each device to raise at most one interrupt at
        // a time; tell the PLIC the device is now allowed to interrupt
        // again (trap.c:206-210).
        if let Some(irq) = irq {
            intr::complete(hart, irq);
        }
        return 1;
    }

    if scause == SCAUSE_SUP_TIMER {
        // Timer interrupt (trap.c:213-216).
        clockintr();
        return 2;
    }

    0
}
