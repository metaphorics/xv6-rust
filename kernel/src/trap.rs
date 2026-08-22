//! Trap handling (`kernel/trap.c`) — the kernel-side half. The vector
//! itself is assembly ([`arch::riscv64::kernelvec`]); the user-mode half
//! (uservec/userret, usertrap) joins with processes (M4).

use crate::arch;
use crate::arch::riscv64::{self, intr, kernelvec};
use crate::dev::uart16550;
use crate::sync::SpinLock;

/// sstatus.SPP: previous mode, 1 = supervisor (riscv.h:44).
const SSTATUS_SPP: usize = 1 << 8;

/// Supervisor external interrupt, the full scause value (trap.c:192).
const SCAUSE_SUP_EXTERNAL: usize = 0x8000_0000_0000_0009;

/// Supervisor timer interrupt, the full scause value (trap.c:213).
const SCAUSE_SUP_TIMER: usize = 0x8000_0000_0000_0005;

/// The tick counter and its lock (`tickslock`, `ticks`, trap.c:9-10).
/// A const-constructed `SpinLock` replaces `trapinit`'s `initlock`
/// (trap.c:19-23).
static TICKS: SpinLock<u64> = SpinLock::new(0);

/// Set up to take exceptions and traps while in the kernel
/// (`trapinithart`, trap.c:26-30): install the kernel trap vector.
pub fn init_hart() {
    riscv64::w_stvec(kernelvec::addr());
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

    if devintr() == 0 {
        // interrupt or trap from an unknown source (trap.c:150-153).
        println!(
            "scause={:#x} sepc={:#x} stval={:#x}",
            scause,
            riscv64::r_sepc(),
            riscv64::r_stval()
        );
        panic!("kerneltrap");
    }

    // give up the CPU if this is a timer interrupt (trap.c:156-158): the
    // yield needs a current process and joins with the scheduler (M4).

    // the yield() may have caused some traps to occur, so restore trap
    // registers for use by kernelvec's sret (trap.c:160-163).
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
        // wakeup(&ticks) (trap.c:172) joins when sleep/wakeup exist
        // (M4); nothing reads ticks before the first sys_sleep /
        // sys_uptime, so no waiter can be missed.
    }

    // ask for the next timer interrupt. this also clears the interrupt
    // request (trap.c:176-179).
    riscv64::w_stimecmp(riscv64::r_time() + riscv64::TIMER_INTERVAL);
}

/// Check if it's an external interrupt or software interrupt, and handle
/// it. Returns 2 if timer interrupt, 1 if other device, 0 if not
/// recognized (`devintr`, trap.c:187-220).
fn devintr() -> u32 {
    let scause = riscv64::r_scause();

    if scause == SCAUSE_SUP_EXTERNAL {
        // this is a supervisor external interrupt, via PLIC (trap.c:193).
        let hart = arch::cpu_id();

        // irq indicates which device interrupted (trap.c:196).
        let irq = intr::claim(hart);
        match irq {
            Some(intr::UART0_IRQ) => uart16550::intr(),
            // virtio_disk_intr() (trap.c:200-201) joins with the disk
            // driver (M5); an unconfigured virtio device never raises
            // its IRQ, so this arm reports rather than dispatches.
            Some(irq) => println!("unexpected interrupt irq={}", irq),
            None => {}
        }

        // the PLIC allows each device to raise at most one interrupt at a
        // time; tell the PLIC the device is now allowed to interrupt
        // again (trap.c:206-210).
        if let Some(irq) = irq {
            intr::complete(hart, irq);
        }
        return 1;
    }

    if scause == SCAUSE_SUP_TIMER {
        // timer interrupt (trap.c:213-216).
        clockintr();
        return 2;
    }

    0
}
